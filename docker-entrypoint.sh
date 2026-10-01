#!/bin/sh
# Native services have their own image. This entrypoint starts only the Svelte renderer.
set -e
exec node ./build/index.js
