# agent sandbox

same model as hexmapper's sandbox: agents get a dedicated clone under
~/agents, mounted alone in a container with default-deny egress (LLM APIs,
github, npm, crates.io allowlisted) and no credentials beyond a repo-scoped
PAT. launch with `~/agents/start` (or `~/agents/up` for just the container).

tabula-specific notes:

- toolchain inside: rust + wasm32-unknown-unknown + wasm-tools (so
  `plugins/build.sh` works), node 20, psql, gh, the three agent CLIs.
- `DATABASE_URL` points at `tabula_test` on the host supabase postgres
  (host.docker.internal:54322), so DB-gated `cargo test` runs for real.
  `SUPABASE_URL` points at the host stack for JWKS if you boot the kernel.
- named volumes for `target/`, `web/node_modules`, and the cargo registry;
  the first two are namespaced per clone, the registry is shared.
- dev flow: branch off `staging`, PR into `staging`. `main` is the release
  branch and only fast-forwards from staging when I say so. both are
  update-restricted by ruleset; the agent PAT can't touch either.
