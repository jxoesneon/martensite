# Martensite Fuzz Soak Campaign (Local Docker)

Runs the deterministic 48-hour fuzz soak campaign inside a Docker
container. This setup is for local use only; it is not wired into CI.

## Quick start

```sh
cd docker/fuzz
docker compose up --build
```

Results are written to `docker/fuzz/results/fuzz-results.txt`.

## Configuration

All variables have sensible defaults; override any of them on the
`docker compose` command line or in a `.env` file next to the compose file.

| Variable                    | Default  | Description                          |
|-----------------------------|----------|--------------------------------------|
| `MARTENSITE_FUZZ_SEED`      | `1`      | Deterministic seed for the campaign. |
| `MARTENSITE_FUZZ_DURATION`  | `172800` | Wall-clock budget in seconds (48 h).  |
| `MARTENSITE_FUZZ_RELEASE`   | `1`      | `1` = release build, `0` = dev build.|

### Short smoke run (5 minutes)

```sh
MARTENSITE_FUZZ_DURATION=300 docker compose up --build
```

### Custom seed

```sh
MARTENSITE_FUZZ_SEED=42 docker compose up --build
```

## What it does

1. Builds the `martensite-test` fuzz test binary in release mode.
2. Runs the `soak_campaign` ignored test, which exercises all three fuzz
   targets (ArenaCompaction, ReactiveDag, EventRouting) for the configured
   duration.
3. Logs all output to `/tmp/fuzz-results/fuzz-results.txt` inside the
   container, mapped to `./results/` on the host.
4. Exits 0 on success, non-zero on any invariant failure.

## Running without Docker

```sh
MARTENSITE_FUZZ_SEED=1 MARTENSITE_FUZZ_DURATION=300 \
    cargo test -p martensite-test --lib --release soak_campaign -- --ignored --nocapture
```
