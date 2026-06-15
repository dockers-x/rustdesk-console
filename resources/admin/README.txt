Admin SPA build output goes here (embedded into the binary by rust-embed).
Build it from ../web:  cd web && npm install && npx vite build
CI and the Dockerfile do this automatically. This placeholder keeps the
folder present in a fresh checkout so the Rust build can embed it.
