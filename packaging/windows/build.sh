#!/usr/bin/env bash
set -euo pipefail

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN* | Windows*)
    pwsh -ExecutionPolicy Bypass -File "$(dirname "$0")/build.ps1"
    ;;
  *)
    exec "$(dirname "$0")/build-cross.sh"
    ;;
esac
