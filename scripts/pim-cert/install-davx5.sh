#!/usr/bin/env sh
set -eu

command -v adb >/dev/null 2>&1 || {
  echo "adb is required"
  exit 1
}

if adb shell pm list packages | grep -Eq 'at\.bitfire\.(davdroid|davx5)'; then
  echo "DAVx5 is already installed"
  adb shell pm list packages | grep -E 'at\.bitfire\.(davdroid|davx5)'
  exit 0
fi

if [ -z "${DAVX5_APK:-}" ]; then
  echo "DAVx5 is not installed and DAVX5_APK is not set"
  echo "Download the DAVx5 APK through F-Droid or another approved source, then run:"
  echo "DAVX5_APK=/path/to/davx5.apk scripts/pim-cert/install-davx5.sh"
  exit 1
fi

adb install -r "$DAVX5_APK"
adb shell pm list packages | grep -E 'at\.bitfire\.(davdroid|davx5)'
