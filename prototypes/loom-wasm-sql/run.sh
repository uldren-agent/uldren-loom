#!/bin/bash
# Build the wasm SQL binding and serve the TWO-ORIGIN demo:
#   X (vendor CDN)      = http://localhost:8000  -> serves web/   (wasm, engine, coordinator, loom.js)
#   Y (integrator site) = http://localhost:8001  -> serves integrator/ (page + 1-line coordinator stub)
# The integrator page loads everything from X cross-origin; the workers run in Y's origin (Y's OPFS).
#
#   ./run.sh                  # build, serve both, open the integrator (Y) page
#   SKIP_BUILD=1 ./run.sh     # reuse the existing web/pkg
#   XPORT=8000 YPORT=8001 ./run.sh
# Needs: wasm-pack, python3. Keep XPORT=8000 unless you also change integrator/loom-coordinator.js and
# pass ?cdn=http://localhost:<XPORT> to the integrator page.
set -euo pipefail
cd "$(dirname "$0")"

if [ -z "${SKIP_BUILD:-}" ]; then
  ( cd ../../bindings/wasm && wasm-pack build --target web --out-dir ../../prototypes/loom-wasm-sql/web/pkg )
fi

XPORT="${XPORT:-8000}"
YPORT="${YPORT:-8001}"

python3 cors-server.py "$XPORT" web &        X=$!
python3 cors-server.py "$YPORT" integrator & Y=$!
trap 'kill "$X" "$Y" 2>/dev/null || true' EXIT
sleep 1

echo
echo "  X (vendor CDN)      http://localhost:$XPORT/            (also a same-origin demo at /index.html)"
echo "  Y (integrator site) http://localhost:$YPORT/            <- open this; loads Loom from X"
echo "  Ctrl-C to stop."
echo
URL="http://localhost:$YPORT/"
if command -v open >/dev/null 2>&1; then open "$URL"
elif command -v xdg-open >/dev/null 2>&1; then xdg-open "$URL"
else echo "open this in a browser: $URL"; fi

wait
