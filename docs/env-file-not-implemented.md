# Why mk doesn't load `.env` into recipe environment

## The request

A common pattern in mkfiles:

```mkfile
ENV_FILE=.env
TOOL = ./scripts/run.sh

data/output.json: data/input.csv .env
	$TOOL --input $prereq --output $target
```

The user expects `ENV_FILE=.env` to mean: *"read `.env` and inject its `KEY=VALUE` pairs into the environment of every recipe subprocess."*

After all, `ENV_FILE` sounds like it should do exactly that — and `.env` is a universal convention (Docker, systemd, Node.js, Python, direnv).

**Reality:** `ENV_FILE=.env` is just a regular mk variable with value `".env"`. mk does NOT parse the file or export its contents. The `.env` prerequisite only causes cache invalidation when the file changes — its contents never reach recipe subprocesses.

## Why we said no

### 1. Not in the Plan 9 mk spec

mk already imports the OS environment at startup (F-040) and exports all variables to recipe subprocesses (F-064). Adding file-based env loading is a new concept outside the original specification. Every feature in mk-rust maps to an F-number in `docs/mk-spec.md` — this one doesn't.

### 2. The workaround is simple and correct

**Option A: Shell-level sourcing (for sh recipes)**

```mkfile
data/output.json: data/input.csv .env
	set -a; . .env; set +a; ./scripts/run.sh --input $prereq --output $target
```

**Option B: Application-level reading (for Rust/Python/Node tools)**

```rust
// In your tool's main.rs — 5 lines, no magic
fn load_env_file(path: &str) {
    if let Ok(content) = std::fs::read_to_string(path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if let Some((k, v)) = line.split_once('=') {
                std::env::set_var(k.trim(), v.trim());
            }
        }
    }
}
```

Both are explicit, debuggable, and don't require mk to understand `.env` syntax.

### 3. `.env` parsing is a mini-specification

If mk were to parse `.env` files, it would need to decide:

| Ambiguity | Example |
|-----------|---------|
| Comments | `# this is a comment` — strip or keep? |
| `export` prefix | `export FOO=bar` vs `FOO=bar` |
| Quoted values | `TOKEN="value with spaces"` |
| Empty lines | Skip silently? |
| Inline comments | `FOO=bar # trailing` — value is `bar` or `bar # trailing`? |
| Multi-line values | `CERT="-----BEGIN...\n...-----"` |
| Variable expansion | `DB_URL=postgres://$HOST/db` — expand `$HOST`? |

There's no universal `.env` standard. Docker, docker-compose, systemd EnvironmentFile, Python-dotenv, and direnv all parse slightly differently. Choosing one interpretation creates a bug surface for all the others.

### 4. Precedence ambiguity

If mk loads `.env`, what's the priority?

```
OS environment  >  .env file  >  mkfile assignments
```

or

```
OS environment  >  mkfile assignments  >  .env file
```

If `.env` is lowest precedence (safer — can't override system vars), users are confused when `TOKEN` in `.env` doesn't override a system `TOKEN`. If `.env` is highest, it's a security problem (a compromised `.env` can hijack `PATH`).

### 5. Scope: environment management is not mk's job

mk is a **dependency-driven executor**, not an environment manager. Mixing these concerns:

- Makes mk harder to understand (another special variable with magic behavior)
- Duplicates functionality already available in direnv, dotenv-cli, shell `source`
- Creates coupling between a build tool and a deployment convention

The Unix way: compose small tools. Load `.env` with a tool designed for it, pipe the result to mk.

```bash
# Before running mk: let the shell or direnv handle it
source .env && mk all

# Or: use env to inject variables
env $(grep -v '^#' .env | xargs) mk all

# Or: let direnv auto-load on cd
echo 'dotenv' > .envrc && direnv allow
```

## The "not implementing a feature is hard" principle

Every feature has a lifetime cost:

- **Maintenance:** code to read, test, and not break during refactors
- **Documentation:** users must learn what `ENV_FILE` does and doesn't do
- **Interaction:** how does `.env` loading interact with includes, backtick expansion, recursive variables?
- **Migration:** if we add it and later change semantics, downstream mkfiles break

When the workaround is 5 lines in user code, the feature doesn't earn its keep.

## What mk DOES export to recipes

mk already exports ALL variables in scope to recipe subprocesses (F-064):

```mkfile
API_URL = https://api.example.com
TOKEN = sk-abc123

fetch-data:
	curl -H "Authorization: Bearer $TOKEN" $API_URL/data > $target
```

Variables set in the mkfile, on the command line (`mk TOKEN=xyz`), or imported from OS environment — all reach the recipe's subprocess. The gap is ONLY file-based variable loading, which the workarounds above handle cleanly.

## Related

- `docs/mk-spec.md` §5.8 — Environment and Recipes (F-064)
- `docs/mk-spec.md` §4.1 — Variable System (F-040, F-041, F-042)
- `AGENTS.md` — Gotchas section
