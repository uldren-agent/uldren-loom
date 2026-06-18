#!/usr/bin/env sh
set -eu

HOSTNAME="${PIM_CERT_HOSTNAME:-uldrentest.com}"
ANDROID_HOST_IP="${PIM_CERT_ANDROID_HOST_IP:-10.0.2.2}"

command -v adb >/dev/null 2>&1 || {
  echo "adb is required"
  exit 1
}

adb devices
adb root
adb remount
adb pull /system/etc/hosts /tmp/loom-pim-cert-android-hosts

if grep -q "[[:space:]]$HOSTNAME" /tmp/loom-pim-cert-android-hosts; then
  awk -v host="$HOSTNAME" -v ip="$ANDROID_HOST_IP" '
    $2 == host { print ip " " host; next }
    { print }
  ' /tmp/loom-pim-cert-android-hosts > /tmp/loom-pim-cert-android-hosts.new
else
  cat /tmp/loom-pim-cert-android-hosts > /tmp/loom-pim-cert-android-hosts.new
  printf '%s %s\n' "$ANDROID_HOST_IP" "$HOSTNAME" >> /tmp/loom-pim-cert-android-hosts.new
fi

adb push /tmp/loom-pim-cert-android-hosts.new /system/etc/hosts
adb shell cat /system/etc/hosts

echo "Android emulator now maps $HOSTNAME to $ANDROID_HOST_IP"
