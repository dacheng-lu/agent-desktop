#!/bin/sh
set -eu

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/ad macos vm provider.XXXXXX")
cleanup() {
  for pid in "${source_pid:-}" "${foreign_pid:-}" "${vm_pid:-}" "${job_pid:-}" "${healthy_pid:-}" "${failed_pid:-}"; do
    [ -n "$pid" ] || continue
    kill "$pid" 2>/dev/null || true
  done
  chmod -R u+rwX "$tmp" 2>/dev/null || true
  rm -rf "$tmp"
}
trap cleanup 0
trap 'exit 1' 1 2 3 15
mkdir -p "$tmp/bin" "$tmp/provider/build" "$tmp/sandboxes/demo"

AD_SANDBOX_DIR=$tmp/sandboxes/demo
AD_PROVIDER_DIR=$tmp/provider
AD_BINARY=$tmp/agent-desktop
VMHOST=$tmp/bin/vmhost
FAKE_LOG=$tmp/calls
FAKE_HV_SUPPORT=1
FAKE_SWIFTC_FOUND=1
FAKE_GUEST_SHA1=2222222222222222222222222222222222222222
FAKE_GUEST_HELPER_SHA1=3333333333333333333333333333333333333333
FAKE_EXEC_BUDGET=300
FAKE_INSTALLS=$tmp/installs
FAKE_PS_COMM=/opt/agent-desktop/build/vmhost
FAKE_PS_BUNDLE=$AD_SANDBOX_DIR/vm
FAKE_PS_COMMAND=
export AD_SANDBOX_DIR AD_PROVIDER_DIR AD_BINARY VMHOST FAKE_LOG
export FAKE_HV_SUPPORT FAKE_SWIFTC_FOUND FAKE_GUEST_SHA1 FAKE_GUEST_HELPER_SHA1
export FAKE_EXEC_BUDGET FAKE_INSTALLS
export FAKE_PS_BUNDLE FAKE_PS_COMM FAKE_PS_COMMAND

printf '#!/bin/sh\n' > "$AD_PROVIDER_DIR/guest-setup.sh"
printf 'binary\n' > "$AD_BINARY"
printf 'helper\n' > "$tmp/agent-desktop-macos-helper"
chmod 0755 "$tmp/agent-desktop-macos-helper"

cat > "$tmp/bin/uname" <<'FAKE'
#!/bin/sh
printf '%s\n' arm64
FAKE
cat > "$tmp/bin/sw_vers" <<'FAKE'
#!/bin/sh
printf '%s\n' 14.0
FAKE
cat > "$tmp/bin/sysctl" <<'FAKE'
#!/bin/sh
printf '%s\n' "${FAKE_HV_SUPPORT:-1}"
FAKE
cat > "$tmp/bin/xcrun" <<'FAKE'
#!/bin/sh
if [ "${1:-}" = --find ]; then
  [ "${FAKE_SWIFTC_FOUND:-1}" = 1 ] || exit 1
  printf '%s\n' /usr/bin/swiftc
  exit 0
fi
exit 2
FAKE
cat > "$tmp/bin/codesign" <<'FAKE'
#!/bin/sh
printf 'com.apple.security.virtualization\n'
FAKE
cat > "$tmp/bin/vmhost" <<'FAKE'
#!/bin/sh
set -eu
{
  printf 'BEGIN vmhost\n'
  for arg in "$@"; do printf '%s\n' "$arg"; done
  printf 'END\n'
} >> "$FAKE_LOG"
case ${1:-} in
  install)
    printf '%s\n' "$3" >> "$FAKE_INSTALLS"
    ;;
  clone)
    mkdir -p "$3/share"
    : > "$3/Disk.img"
    : > "$3/AuxiliaryStorage"
    : > "$3/HardwareModel"
    : > "$3/MachineIdentifier"
    : > "$3/config.json"
    ;;
  ip) printf '{"ok":true,"ip":"192.0.2.2"}\n' ;;
  run)
    case ${FAKE_RUN_BEHAVIOR:-} in
      boot-fails)
        printf '%s\n' "$$" > "$2/vmhost.pid"
        sleep 1
        rm -f "$2/vmhost.pid"
        exit 1
        ;;
      boots)
        printf '%s\n' "$$" > "$2/vmhost.pid"
        sleep 1
        : > "$2/vmhost.started"
        exec sleep 30
        ;;
      *) exit 2 ;;
    esac
    ;;
  *) exit 2 ;;
esac
FAKE
cat > "$tmp/bin/ssh-keygen" <<'FAKE'
#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
  if [ "$1" = -f ]; then shift; key=$1; fi
  shift
done
printf 'private key\n' > "$key"
printf 'public key\n' > "$key.pub"
FAKE
cat > "$tmp/bin/curl" <<'FAKE'
#!/bin/sh
set -eu
{
  printf 'BEGIN curl\n'
  for arg in "$@"; do printf '%s\n' "$arg"; done
  printf 'END\n'
} >> "$FAKE_LOG"
out=
head=
prev=
for arg in "$@"; do
  case $prev in -o) out=$arg ;; esac
  case $arg in -*I*) head=1 ;; esac
  prev=$arg
done
if [ -n "$out" ]; then
  case $out in
    */catalog.xml) printf '%s' "${FAKE_CATALOG_XML:?}" > "$out"; exit 0 ;;
  esac
  case ${AD_SANDBOX_DIR:-} in
    */concurrent/healthy)
      image=${FAKE_IMAGE_CONTENT:?}
      image_prefix=${image%?}
      image_suffix=${image#"$image_prefix"}
      exec 3> "$out"
      printf '%s' "$image_prefix" >&3
      : > "${FAKE_CURL_SLOW_STARTED:?}"
      slow_tries=0
      while [ ! -e "${FAKE_CURL_RELEASE:?}" ] && [ "$slow_tries" -lt 100 ]; do
        sleep 0.1
        slow_tries=$((slow_tries + 1))
      done
      [ -e "$FAKE_CURL_RELEASE" ] || exit 1
      printf '%s' "$image_suffix" >&3
      exec 3>&-
      exit 0
      ;;
    */concurrent/failed)
      printf 'partial' > "$out"
      : > "${FAKE_CURL_FAILED:?}"
      exit 22
      ;;
  esac
  if [ "${FAKE_CURL_GET_FAIL:-0}" = 1 ]; then exit 22; fi
  printf '%s' "${FAKE_IMAGE_CONTENT:?}" > "$out"
  exit 0
fi
if [ -n "$head" ]; then
  printf 'HTTP/1.1 200 OK\r\nContent-Length: %s\r\n' "${#FAKE_IMAGE_CONTENT}"
  exit 0
fi
exit 1
FAKE
cat > "$tmp/bin/ssh" <<'FAKE'
#!/bin/sh
set -eu
{
  printf 'BEGIN ssh\n'
  for arg in "$@"; do printf '%s\n' "$arg"; done
  printf 'END\n'
} >> "$FAKE_LOG"
remote=
batch=
connect_timeout=
alive_interval=
alive_count=
for arg in "$@"; do
  remote=$arg
  case $arg in
    BatchMode=yes) batch=1 ;;
    ConnectTimeout=*) connect_timeout=${arg#*=} ;;
    ServerAliveInterval=*) alive_interval=${arg#*=} ;;
    ServerAliveCountMax=*) alive_count=${arg#*=} ;;
  esac
done
if [ -z "$batch" ]; then sleep 40; exit 255; fi
case ${FAKE_SSH_BEHAVIOR:-echo} in
  granted)
    printf '{"ok":true,"data":{"accessibility":{"state":"granted"}}}\n'
    exit 0
    ;;
  unreachable)
    if [ -z "$connect_timeout" ]; then sleep 40; fi
    exit 255
    ;;
  silent)
    if [ -z "$alive_interval" ] || [ -z "$alive_count" ]; then sleep 40; exit 255; fi
    sleep "$((alive_interval * alive_count))"
    exit 255
    ;;
  quiet)
    cutoff=0
    if [ -n "$alive_interval" ] && [ -n "$alive_count" ]; then
      cutoff=$((alive_interval * alive_count))
    fi
    case $remote in
      *'shasum -a 1'*|*'mkdir -p'*|*'chmod 755'*) ;;
      *)
        if [ "$cutoff" -gt 0 ] && [ "$cutoff" -lt "${FAKE_EXEC_BUDGET:-300}" ]; then
          exit 255
        fi
        printf 'guest output\n'
        exit 0
        ;;
    esac
    ;;
esac
case $remote in
  *'shasum -a 1 ~/.local/bin/agent-desktop-macos-helper'*)
    [ -n "${FAKE_GUEST_HELPER_SHA1:-}" ] || exit 1
    printf '%s  agent-desktop-macos-helper\n' "${FAKE_GUEST_HELPER_SHA1:-}"
    ;;
  *'shasum -a 1'*) printf '%s  agent-desktop\n' "${FAKE_GUEST_SHA1:-}" ;;
  *) printf '{"ok":true,"command":"guest"}\n' ;;
esac
FAKE
cat > "$tmp/bin/scp" <<'FAKE'
#!/bin/sh
set -eu
{
  printf 'BEGIN scp\n'
  for arg in "$@"; do printf '%s\n' "$arg"; done
  printf 'END\n'
} >> "$FAKE_LOG"
batch=
connect_timeout=
for arg in "$@"; do
  case $arg in
    BatchMode=yes) batch=1 ;;
    ConnectTimeout=*) connect_timeout=${arg#*=} ;;
  esac
done
if [ -z "$batch" ] || [ -z "$connect_timeout" ]; then
  sleep 40
  exit 1
fi
FAKE
cat > "$tmp/bin/ps" <<'FAKE'
#!/bin/sh
case " $* " in
  *" command="*) printf '%s\n' "${FAKE_PS_COMMAND:-/opt/agent-desktop/build/vmhost run $FAKE_PS_BUNDLE}" ;;
  *) printf '%s\n' "${FAKE_PS_COMM:-/opt/agent-desktop/build/vmhost}" ;;
esac
FAKE
chmod 0755 "$tmp/bin/"*
PATH=$tmp/bin:$PATH
export PATH

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

run_provider() {
  sh "$here/provider.sh" "$@"
}

run_provider_with() {
  setting=$1
  value=$2
  shift 2
  env "$setting=$value" sh "$here/provider.sh" "$@"
}

wait_for_exit() {
  wait_tries=0
  while kill -0 "$1" 2>/dev/null && [ "$wait_tries" -lt 100 ]; do
    sleep 0.1
    wait_tries=$((wait_tries + 1))
  done
}

assert_field() {
  actual=$(python3 -c 'import json,sys; print(json.loads(open(sys.argv[1]).readlines()[-1])[sys.argv[2]])' "$1" "$2")
  [ "$actual" = "$3" ] || fail "$4: expected $2=[$3] but saw [$actual]"
}

run_ok() {
  status=0
  run_provider "$@" > "$tmp/out" 2> "$tmp/err" || status=$?
  [ "$status" = 0 ] || fail "provider $1 exited $status: $(cat "$tmp/out" "$tmp/err")"
}

run_ok_with() {
  setting=$1
  value=$2
  verb=$3
  shift 3
  status=0
  run_provider_with "$setting" "$value" "$verb" "$@" > "$tmp/out" 2> "$tmp/err" || status=$?
  [ "$status" = 0 ] || fail "provider $verb exited $status: $(cat "$tmp/out" "$tmp/err")"
}

FAKE_HV_SUPPORT=0
run_ok check
assert_field "$tmp/out" state unavailable 'a Mac that cannot run VMs'
FAKE_HV_SUPPORT=1
FAKE_SWIFTC_FOUND=0
run_ok check
assert_field "$tmp/out" state unavailable 'missing Command Line Tools'
run_ok install
assert_field "$tmp/out" state unavailable 'install without Command Line Tools'
FAKE_SWIFTC_FOUND=1

locked_dir="$tmp/locked\\dir \"quoted\""
mkdir -p "$locked_dir"
chmod 0555 "$locked_dir"
status=0
run_provider_with AD_PROVIDER_DIR "$locked_dir" install > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "install into an unwritable directory exited $status: $(cat "$tmp/out" "$tmp/err")"
assert_field "$tmp/out" suggestion "Make $locked_dir writable, then run agent-desktop sandbox install macos." \
  'a provider path holding a backslash and a quote'

printf '#!/bin/sh\nexit 0\n' > "$AD_PROVIDER_DIR/build/vmhost"
chmod 0755 "$AD_PROVIDER_DIR/build/vmhost"
: > "$FAKE_LOG"
run_ok install
assert_field "$tmp/out" state ready 'a built and signed VM host'
FAKE_SWIFTC_FOUND=0
run_ok check
assert_field "$tmp/out" state ready 'check with a ready host and no Command Line Tools'
FAKE_SWIFTC_FOUND=1

cache_dir=$tmp/cache/macos
FAKE_IMAGE_CONTENT='fake macos image payload
'
image_sha=$(printf '%s' "$FAKE_IMAGE_CONTENT" | shasum -a 1 | awk '{ print toupper($1) }')
image_key=$(printf '%s' "$image_sha" | tr 'A-F' 'a-f')
image_path=$cache_dir/$image_key.ipsw
FAKE_CATALOG_XML="<?xml version=\"1.0\"?>
<plist><dict><key>Restore</key><dict>
<key>FirmwareURL</key><string>https://example.test/UniversalMac_15.0.ipsw</string>
<key>FirmwareSHA1</key><string>$image_sha</string>
<key>ProductVersion</key><string>15.0</string>
</dict></dict></plist>
"
export FAKE_IMAGE_CONTENT FAKE_CATALOG_XML

run_download_job() {
  download_dir=$1
  mkdir -p "$download_dir/vm" "$cache_dir"
  printf 'public key\n' > "$download_dir/id_ed25519.pub"
  : > "$FAKE_INSTALLS"
  download_status=0
  AD_SANDBOX_DIR=$download_dir sh "$here/provider.sh" __install_job download '' "$cache_dir" guest > "$tmp/out" 2> "$tmp/err" || download_status=$?
}

printf '%s' "$FAKE_IMAGE_CONTENT" > "$tmp/expected-image"
mkdir -p "$cache_dir"
printf '%s' "$FAKE_IMAGE_CONTENT" > "$image_path"
: > "$FAKE_LOG"
run_download_job "$tmp/downloads/complete"
[ "$download_status" = 0 ] || fail "a cached image with a matching SHA-1 failed: $(cat "$tmp/out" "$tmp/err")"
grep -qx "$image_path" "$FAKE_INSTALLS" || fail 'the verified cached image was not installed'
if grep -qx -- '-C' "$FAKE_LOG"; then
  fail "a complete cached image with a matching SHA-1 was downloaded again: $(cat "$FAKE_LOG")"
fi

partial_path=$tmp/downloads/partial/vm/download/UniversalMac_15.0.ipsw
rm -f "$image_path"
mkdir -p "$(dirname "$partial_path")"
printf 'partial' > "$partial_path"
: > "$FAKE_LOG"
run_download_job "$tmp/downloads/partial"
[ "$download_status" = 0 ] || fail "a partial cached image did not resume: $(cat "$tmp/out" "$tmp/err")"
grep -qx -- '-C' "$FAKE_LOG" || fail 'a partial cached image did not resume with -C -'
grep -qx "$image_path" "$FAKE_INSTALLS" || fail 'the resumed image was not installed'
cmp -s "$image_path" "$tmp/expected-image" || fail 'the resumed image does not match the catalog SHA-1'
[ ! -f "$partial_path" ] || fail 'a published download left its private partial image'
[ ! -e "$tmp/downloads/partial/vm/download" ] || fail 'the install ran with the download directory still in the bundle'

printf '%*s' "${#FAKE_IMAGE_CONTENT}" '' | tr ' ' x > "$image_path"
: > "$FAKE_LOG"
run_download_job "$tmp/downloads/mismatch"
[ "$download_status" = 0 ] || fail "a corrupt cached image was not replaced: $(cat "$tmp/out" "$tmp/err")"
grep -qx "$image_path" "$FAKE_INSTALLS" || fail 'the replaced image was not installed'
cmp -s "$image_path" "$tmp/expected-image" || fail 'a corrupt cached image was installed instead of a verified download'

rm -f "$image_path"
FAKE_CURL_GET_FAIL=1
export FAKE_CURL_GET_FAIL
: > "$FAKE_LOG"
run_download_job "$tmp/downloads/failed"
unset FAKE_CURL_GET_FAIL
[ "$download_status" = 1 ] || fail "a failed download exited $download_status"
[ ! -s "$FAKE_INSTALLS" ] || fail 'a failed download was treated as success and installed'
[ ! -f "$tmp/downloads/failed/vm/install.done" ] || fail 'a failed download recorded a completed install'

concurrent_root=$tmp/concurrent
healthy_dir=$concurrent_root/healthy
failed_dir=$concurrent_root/failed
mkdir -p "$healthy_dir/vm" "$failed_dir/vm"
printf 'public key\n' > "$healthy_dir/id_ed25519.pub"
printf 'public key\n' > "$failed_dir/id_ed25519.pub"
rm -f "$image_path" "$tmp/curl-slow-started" "$tmp/curl-release" "$tmp/curl-failed"
: > "$FAKE_INSTALLS"
FAKE_CURL_SLOW_STARTED=$tmp/curl-slow-started
FAKE_CURL_RELEASE=$tmp/curl-release
FAKE_CURL_FAILED=$tmp/curl-failed
export FAKE_CURL_SLOW_STARTED FAKE_CURL_RELEASE FAKE_CURL_FAILED
AD_SANDBOX_DIR=$healthy_dir sh "$here/provider.sh" __install_job download '' "$cache_dir" guest > "$tmp/healthy.out" 2> "$tmp/healthy.err" &
healthy_pid=$!
wait_tries=0
while [ ! -f "$FAKE_CURL_SLOW_STARTED" ] && [ "$wait_tries" -lt 100 ]; do
  sleep 0.1
  wait_tries=$((wait_tries + 1))
done
[ -f "$FAKE_CURL_SLOW_STARTED" ] || fail 'the healthy concurrent job did not start downloading'
failed_status=0
AD_SANDBOX_DIR=$failed_dir sh "$here/provider.sh" __install_job download '' "$cache_dir" guest > "$tmp/failed.out" 2> "$tmp/failed.err" &
failed_pid=$!
wait "$failed_pid" || failed_status=$?
failed_pid=
: > "$FAKE_CURL_RELEASE"
healthy_status=0
wait "$healthy_pid" || healthy_status=$?
healthy_pid=
[ "$failed_status" = 1 ] || fail "the failing concurrent job exited $failed_status"
[ "$healthy_status" = 0 ] || fail "a concurrent failure broke the healthy job: $(cat "$tmp/healthy.out" "$tmp/healthy.err")"
[ -f "$image_path" ] || fail 'a concurrent failure removed the shared cached image'
cmp -s "$image_path" "$tmp/expected-image" || fail 'the shared cached image was corrupted by a concurrent failure'
grep -qx "$image_path" "$FAKE_INSTALLS" || fail 'the healthy concurrent job did not install the verified shared image'
[ -f "$healthy_dir/vm/install.done" ] || fail 'the healthy concurrent job did not finish'
[ ! -f "$failed_dir/vm/install.done" ] || fail 'the failing concurrent job recorded a completed install'
unset FAKE_CURL_SLOW_STARTED FAKE_CURL_RELEASE FAKE_CURL_FAILED
rm -rf "$tmp/downloads" "$cache_dir" "$concurrent_root"

source_dir=$tmp/sandboxes/source
mkdir -p "$source_dir/vm/share"
: > "$source_dir/vm/Disk.img"
: > "$source_dir/vm/AuxiliaryStorage"
: > "$source_dir/vm/HardwareModel"
: > "$source_dir/vm/MachineIdentifier"
: > "$source_dir/vm/config.json"
printf '#!/bin/sh\n' > "$source_dir/vm/share/guest-setup.sh"
printf 'source public key\n' > "$source_dir/vm/share/id_ed25519.pub"
printf 'binary\n' > "$source_dir/vm/share/agent-desktop"
printf 'source setup marker\n' > "$source_dir/setup_done"
printf 'source key\n' > "$source_dir/id_ed25519"
printf 'source public key\n' > "$source_dir/id_ed25519.pub"
printf 'source hosts\n' > "$source_dir/known_hosts"
printf 'guest\n' > "$source_dir/guest_user"
: > "$FAKE_LOG"
AD_SET_USER=requested
export AD_SET_USER
run_ok_with AD_SET_FROM source create demo
unset AD_SET_USER
python3 - "$tmp/out" "$FAKE_LOG" "$source_dir" "$AD_SANDBOX_DIR" "$tmp/agent-desktop-macos-helper" <<'PY'
import json
import pathlib
import sys

reply, log, source, target, helper = map(pathlib.Path, sys.argv[1:])
assert (
    json.loads(reply.read_text().splitlines()[-1])["state"] == "stopped"
    and "BEGIN curl" not in log.read_text().splitlines()
    and "clone" in log.read_text().splitlines()
    and all((target / name).read_bytes() == (source / name).read_bytes()
            for name in ("id_ed25519", "known_hosts", "setup_done", "guest_user"))
    and (target / "vm/share/agent-desktop-macos-helper").read_bytes() == helper.read_bytes()
), "create did not clone a set-up stopped source without downloading or copy the helper"
PY

setup_share=$tmp/setup-share
setup_home=$tmp/guest-home
mkdir -p "$setup_share"
cp "$here/guest-setup.sh" "$setup_share/guest-setup.sh"
cp "$tmp/agent-desktop-macos-helper" "$setup_share/agent-desktop-macos-helper"
printf 'public key\n' > "$setup_share/id_ed25519.pub"
printf '#!/bin/sh\nprintf "requested\\n" > "$HOME/permissions-requested"\n' > "$setup_share/agent-desktop"
chmod 0755 "$setup_share/agent-desktop"
HOME=$setup_home sh "$setup_share/guest-setup.sh" > "$tmp/out" 2> "$tmp/err" || \
  fail "guest setup failed: $(cat "$tmp/out" "$tmp/err")"
[ -x "$setup_home/.local/bin/agent-desktop" ] || fail 'guest setup did not install agent-desktop'
cmp -s "$setup_home/.local/bin/agent-desktop-macos-helper" "$tmp/agent-desktop-macos-helper" || \
  fail 'guest setup did not install the helper beside agent-desktop'
[ ! -e "$setup_home/.local/bin/agent-desktop-macos-helper.new" ] || fail 'guest setup left a staged helper'
[ -f "$setup_home/permissions-requested" ] || fail 'guest setup did not request permissions from the installed binary'

sleep 30 &
source_pid=$!
printf '%s\n' "$source_pid" > "$source_dir/vm/vmhost.pid"
FAKE_PS_COMMAND="/opt/agent-desktop/build/vmhost run $source_dir/vm"
rm -rf "$AD_SANDBOX_DIR"
mkdir -p "$AD_SANDBOX_DIR"
status=0
run_provider_with AD_SET_FROM source create demo > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "create from a running source exited $status"
kill -0 "$source_pid" 2>/dev/null || fail 'create signalled the running source'
kill "$source_pid" 2>/dev/null || true
wait "$source_pid" 2>/dev/null || true
source_pid=
rm -f "$source_dir/vm/vmhost.pid"
FAKE_PS_COMMAND=

mv "$tmp/agent-desktop-macos-helper" "$tmp/helper.off"
: > "$FAKE_LOG"
status=0
run_provider create demo > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "create without the host helper exited $status"
grep -q 'agent-desktop-macos-helper' "$tmp/out" || fail 'create did not name the missing host helper'
grep -q 'BEGIN curl' "$FAKE_LOG" && fail 'create without the host helper started a download'
grep -qx 'clone' "$FAKE_LOG" && fail 'create without the host helper cloned a source'
mv "$tmp/helper.off" "$tmp/agent-desktop-macos-helper"

printf '999999999\n' > "$source_dir/vm/vmhost.pid"
: > "$FAKE_LOG"
run_ok create demo
assert_field "$tmp/out" state stopped 'automatic create over a dead source VM record'
[ ! -f "$source_dir/vm/vmhost.pid" ] || fail 'automatic create kept the dead source VM record'
grep -qx 'clone' "$FAKE_LOG" || fail 'automatic create did not clone the source'
grep -q 'BEGIN curl' "$FAKE_LOG" && fail 'automatic create downloaded instead of cloning a dead-but-ready source'

rm -rf "$AD_SANDBOX_DIR"
mkdir -p "$AD_SANDBOX_DIR"
sleep 30 &
source_pid=$!
printf '%s\n' "$source_pid" > "$source_dir/vm/vmhost.pid"
FAKE_PS_COMM=/opt/agent-desktop/build/vmhost
FAKE_PS_COMMAND="/opt/agent-desktop/build/vmhost run $source_dir/vm"
: > "$FAKE_LOG"
status=0
run_provider create demo > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "automatic create from a running source exited $status"
grep -q "Stop source first\." "$tmp/out" || fail 'automatic create did not ask for the source to stop first'
grep -q 'BEGIN curl' "$FAKE_LOG" && fail 'automatic create downloaded while a source was running'
kill -0 "$source_pid" 2>/dev/null || fail 'automatic create signalled the running source'
kill "$source_pid" 2>/dev/null || true
wait "$source_pid" 2>/dev/null || true
source_pid=

sleep 30 &
source_pid=$!
printf '%s\n' "$source_pid" > "$source_dir/vm/vmhost.pid"
FAKE_PS_COMMAND="/opt/agent-desktop/build/vmhost run $tmp/sandboxes/other/vm"
: > "$FAKE_LOG"
status=0
run_provider create demo > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "automatic create from a stale source exited $status"
grep -q 'stale or foreign VM process record' "$tmp/out" || fail 'automatic create did not report the stale source record'
grep -q 'BEGIN curl' "$FAKE_LOG" && fail 'automatic create downloaded while a source record was stale'
kill "$source_pid" 2>/dev/null || true
wait "$source_pid" 2>/dev/null || true
source_pid=
rm -f "$source_dir/vm/vmhost.pid"
FAKE_PS_COMMAND=

mv "$source_dir/vm/Disk.img" "$source_dir/Disk.off"
: > "$FAKE_LOG"
status=0
run_provider create demo > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "automatic create from an incomplete configured source exited $status"
grep -q 'not a completed setup source' "$tmp/out" || fail 'automatic create did not report the incomplete configured source'
grep -q 'BEGIN curl' "$FAKE_LOG" && fail 'automatic create downloaded while a configured source was unavailable'
mv "$source_dir/Disk.off" "$source_dir/vm/Disk.img"

caller_root="$tmp/caller dir"
provider_cwd="$tmp/provider cwd"
relative_image='images/user image.ipsw'
mkdir -p "$caller_root/images" "$provider_cwd"
printf 'caller image\n' > "$caller_root/$relative_image"
rm -rf "$AD_SANDBOX_DIR"
mkdir -p "$AD_SANDBOX_DIR"
: > "$FAKE_INSTALLS"
status=0
(cd "$provider_cwd" && AD_CALLER_CWD="$caller_root" AD_SET_IPSW="$relative_image" \
  sh "$here/provider.sh" create demo) > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 0 ] || fail "a relative image under the caller directory exited $status: $(cat "$tmp/out" "$tmp/err")"
job_pid=$(cat "$AD_SANDBOX_DIR/vm/install.pid")
wait_for_exit "$job_pid"
job_pid=
[ -f "$caller_root/$relative_image" ] || fail 'a relative caller image was not found'
grep -qx "$caller_root/$relative_image" "$FAKE_INSTALLS" || \
  fail 'a relative caller image was not resolved to the caller directory before install'

rm -rf "$AD_SANDBOX_DIR"
mkdir -p "$AD_SANDBOX_DIR"
: > "$FAKE_LOG"
status=0
(cd "$provider_cwd" && unset AD_CALLER_CWD && AD_SET_IPSW="$relative_image" \
  sh "$here/provider.sh" create demo) > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "a relative image without caller context exited $status"
grep -q 'absolute-path' "$tmp/out" || fail 'a relative image without caller context did not ask for an absolute path'
grep -q 'BEGIN curl' "$FAKE_LOG" && fail 'a relative image without caller context started a download'
[ ! -f "$AD_SANDBOX_DIR/vm/install.pid" ] || fail 'a relative image without caller context started an install'

for setting in AD_SET_IPSW AD_SET_FROM AD_SET_USER; do
  empty_sandbox=$tmp/empty-setting/$setting/demo
  mkdir -p "$empty_sandbox"
  : > "$FAKE_LOG"
  status=0
  env "$setting=" AD_SANDBOX_DIR="$empty_sandbox" sh "$here/provider.sh" create demo > "$tmp/out" 2> "$tmp/err" || status=$?
  [ "$status" = 1 ] || fail "create with an empty $setting exited $status: $(cat "$tmp/out" "$tmp/err")"
  assert_field "$tmp/out" code INVALID_ARGS "create with an empty $setting"
  [ ! -f "$empty_sandbox/vm/install.pid" ] || fail "create with an empty $setting started an install"
  grep -q 'BEGIN curl' "$FAKE_LOG" && fail "create with an empty $setting started a download"
done

rm -rf "$AD_SANDBOX_DIR"
mkdir -p "$AD_SANDBOX_DIR"
user_ipsw="$tmp/user image.ipsw"
printf 'user image\n' > "$user_ipsw"
: > "$FAKE_LOG"
FAKE_SWIFTC_FOUND=0
run_ok_with AD_SET_IPSW "$user_ipsw" create demo
job_pid=$(cat "$AD_SANDBOX_DIR/vm/install.pid")
wait_for_exit "$job_pid"
job_pid=
[ -f "$user_ipsw" ] || fail 'a failed installation deleted the user-supplied image'
[ -f "$AD_SANDBOX_DIR/vm/install.done" ] || fail 'create without Command Line Tools did not finish'
grep -qx 'clone' "$FAKE_LOG" && fail 'an explicit image file cloned a configured source instead of installing fresh'
grep -qx "$user_ipsw" "$FAKE_INSTALLS" || fail 'an explicit image file was not installed fresh'

no_clt_dir=$tmp/sandboxes/no-clt
mkdir -p "$no_clt_dir/vm/share"
: > "$no_clt_dir/vm/Disk.img"
: > "$no_clt_dir/vm/AuxiliaryStorage"
: > "$no_clt_dir/vm/HardwareModel"
: > "$no_clt_dir/vm/MachineIdentifier"
: > "$no_clt_dir/vm/config.json"
printf '#!/bin/sh\n' > "$no_clt_dir/vm/share/guest-setup.sh"
printf 'public key\n' > "$no_clt_dir/vm/share/id_ed25519.pub"
cp "$AD_BINARY" "$no_clt_dir/vm/share/agent-desktop"
cp "$tmp/agent-desktop-macos-helper" "$no_clt_dir/vm/share/agent-desktop-macos-helper"
: > "$no_clt_dir/vm/install.done"
: > "$FAKE_LOG"
status=0
run_provider_with AD_SANDBOX_DIR "$no_clt_dir" start no-clt > "$tmp/out" 2> "$tmp/err" || status=$?
grep -q 'Command Line Tools' "$tmp/out" && fail 'start required Command Line Tools with a ready host'
grep -qx 'run' "$FAKE_LOG" || fail "start did not launch the ready VM host without Command Line Tools: $(cat "$tmp/out" "$tmp/err")"
FAKE_SWIFTC_FOUND=1

mkdir -p "$AD_SANDBOX_DIR/vm"
sleep 30 &
foreign_pid=$!
printf '%s\n' "$foreign_pid" > "$AD_SANDBOX_DIR/vm/vmhost.pid"
FAKE_PS_COMM=/usr/bin/sleep
run_ok stop demo
kill -0 "$foreign_pid" 2>/dev/null || fail 'stop signalled a pid whose command was not vmhost'
[ -f "$AD_SANDBOX_DIR/vm/vmhost.pid" ] || fail 'stop discarded a live foreign VM process record'
kill "$foreign_pid" 2>/dev/null || true
wait "$foreign_pid" 2>/dev/null || true
foreign_pid=
sleep 30 &
foreign_pid=$!
printf '%s\n' "$foreign_pid" > "$AD_SANDBOX_DIR/vm/vmhost.pid"
FAKE_PS_COMM=/opt/agent-desktop/build/vmhost
FAKE_PS_COMMAND="/opt/agent-desktop/build/vmhost run $tmp/sandboxes/other/vm"
run_ok stop demo
assert_field "$tmp/out" state broken 'stop of a foreign VM host pid'
kill -0 "$foreign_pid" 2>/dev/null || fail 'stop signalled a vmhost pid that runs another bundle'
[ -f "$AD_SANDBOX_DIR/vm/vmhost.pid" ] || fail 'stop discarded a foreign VM process record for another bundle'
kill "$foreign_pid" 2>/dev/null || true
wait "$foreign_pid" 2>/dev/null || true
foreign_pid=
FAKE_PS_COMMAND=

mkdir -p "$AD_SANDBOX_DIR/vm/share"
: > "$AD_SANDBOX_DIR/vm/Disk.img"
: > "$AD_SANDBOX_DIR/vm/AuxiliaryStorage"
: > "$AD_SANDBOX_DIR/vm/HardwareModel"
: > "$AD_SANDBOX_DIR/vm/MachineIdentifier"
: > "$AD_SANDBOX_DIR/vm/config.json"
printf '#!/bin/sh\n' > "$AD_SANDBOX_DIR/vm/share/guest-setup.sh"
printf 'public key\n' > "$AD_SANDBOX_DIR/vm/share/id_ed25519.pub"
cp "$AD_BINARY" "$AD_SANDBOX_DIR/vm/share/agent-desktop"
rm -f "$AD_SANDBOX_DIR/vm/install.pid" "$AD_SANDBOX_DIR/vm/install.done"
printf '999999999\n' > "$AD_SANDBOX_DIR/vm/vmhost.pid"
run_ok status demo
assert_field "$tmp/out" state stopped 'a dead VM host pid'
[ ! -f "$AD_SANDBOX_DIR/vm/vmhost.pid" ] || fail 'status kept the dead VM process record'
printf '999999999\n' > "$AD_SANDBOX_DIR/vm/vmhost.pid"
: > "$FAKE_LOG"
run_provider start demo > "$tmp/out" 2> "$tmp/err" || true
grep -qx 'run' "$FAKE_LOG" || fail "start did not launch the VM host over a dead record: $(cat "$tmp/out" "$tmp/err")"

rm -f "$AD_SANDBOX_DIR/vm/vmhost.pid" "$AD_SANDBOX_DIR/vm/vmhost.started"
: > "$AD_SANDBOX_DIR/setup_done"
FAKE_RUN_BEHAVIOR=boot-fails
export FAKE_RUN_BEHAVIOR
status=0
run_provider start demo > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "start reported success when the VM host failed to boot: $(cat "$tmp/out" "$tmp/err")"
assert_field "$tmp/out" code ACTION_FAILED 'start of a VM host that failed to boot'
FAKE_RUN_BEHAVIOR=boots
export FAKE_RUN_BEHAVIOR
run_ok start demo
assert_field "$tmp/out" state running 'start of a VM host that booted'
kill "$(cat "$AD_SANDBOX_DIR/vm/vmhost.pid")" 2>/dev/null || true
unset FAKE_RUN_BEHAVIOR
rm -f "$AD_SANDBOX_DIR/vm/vmhost.pid" "$AD_SANDBOX_DIR/vm/vmhost.started" "$AD_SANDBOX_DIR/setup_done"

printf '1000000000\n' > "$AD_SANDBOX_DIR/vm/download.size"
mkfile -n 340000000 "$AD_SANDBOX_DIR/vm/latest.ipsw"
printf '%s\n' "$AD_SANDBOX_DIR/vm/latest.ipsw" > "$AD_SANDBOX_DIR/vm/download.path"
sleep 30 &
job_pid=$!
printf '%s\n' "$job_pid" > "$AD_SANDBOX_DIR/vm/install.pid"
run_ok_with FAKE_PS_COMMAND "$here/provider.sh __install_job download" status demo
grep -q 'is downloading the macOS image: 0.3 of 1.0 GB (34%)\.' "$tmp/out" || \
  fail "status did not report download progress: $(cat "$tmp/out")"
kill "$job_pid" 2>/dev/null || true
wait "$job_pid" 2>/dev/null || true
job_pid=
rm -f "$AD_SANDBOX_DIR/vm/install.pid" "$AD_SANDBOX_DIR/vm/download.size" "$AD_SANDBOX_DIR/vm/download.path"

sleep 30 &
job_pid=$!
printf '%s\n' "$job_pid" > "$AD_SANDBOX_DIR/vm/install.pid"
status=0
run_provider_with FAKE_PS_COMMAND "$here/provider.sh __install_job download" rm demo > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "rm while installing exited $status"
assert_field "$tmp/out" code ACTION_FAILED 'rm while installing'
kill -0 "$job_pid" 2>/dev/null || fail 'rm killed the install job'
[ -d "$AD_SANDBOX_DIR/vm" ] || fail 'rm removed the bundle while the install job was running'
kill "$job_pid" 2>/dev/null || true
wait "$job_pid" 2>/dev/null || true
job_pid=
rm -f "$AD_SANDBOX_DIR/vm/install.pid"
FAKE_PS_COMMAND=

sleep 30 &
vm_pid=$!
printf '%s\n' "$vm_pid" > "$AD_SANDBOX_DIR/vm/vmhost.pid"
: > "$AD_SANDBOX_DIR/setup_done"
weird='semi; double "quoted" and '\''single'\'' $(touch /tmp/should-not-run)'
: > "$FAKE_LOG"
run_provider exec demo -- "$weird" > "$tmp/out" 2> "$tmp/err"
python3 - "$FAKE_LOG" "$weird" <<'PY'
import shlex
import sys

events = []
current = None
for line in open(sys.argv[1]):
    line = line.rstrip("\n")
    if line.startswith("BEGIN ") and current is None:
        current = [line[len("BEGIN "):], []]
    elif line == "END" and current is not None:
        events.append((current[0], current[1]))
        current = None
    elif current is not None:
        current[1].append(line)
ssh = [args for kind, args in events if kind == "ssh"]
scp = [args for kind, args in events if kind == "scp"]
if not ssh:
    sys.exit("no ssh command was recorded")
if shlex.split(ssh[-1][-1]) != ["~/.local/bin/agent-desktop", sys.argv[2]]:
    sys.exit("exec argument was not preserved as one quoted word: %r" % ssh[-1][-1])
commands = [shlex.split(block[-1]) for block in ssh if block]
if ["mkdir", "-p", "~/.local/bin"] not in commands:
    sys.exit("exec did not create ~/.local/bin in the guest")
destinations = [args[-1] for args in scp]
for name in ("agent-desktop", "agent-desktop-macos-helper"):
    if not any(path.endswith(":~/.local/bin/%s.new" % name) for path in destinations):
        sys.exit("exec did not copy %s onto a staging file: %r" % (name, scp))
    if not [words for words in commands
            if words[:3] == ["chmod", "755", "~/.local/bin/%s.new" % name]
            and words[-2:] == ["~/.local/bin/%s.new" % name, "~/.local/bin/%s" % name]]:
        sys.exit("exec did not move the staged %s onto ~/.local/bin/%s" % (name, name))
last_scp = max(index for index, (kind, _) in enumerate(events) if kind == "scp")
last_ssh = max(index for index, (kind, _) in enumerate(events) if kind == "ssh")
if last_scp > last_ssh:
    sys.exit("exec ran the guest command before the files were staged")
PY

FAKE_GUEST_SHA1=$(shasum -a 1 "$AD_BINARY" | awk '{ print tolower($1) }')
FAKE_GUEST_HELPER_SHA1=
export FAKE_GUEST_SHA1 FAKE_GUEST_HELPER_SHA1
: > "$FAKE_LOG"
run_provider exec demo -- snapshot > "$tmp/out" 2> "$tmp/err"
python3 - "$FAKE_LOG" <<'PY'
import sys

events = []
current = None
for line in open(sys.argv[1]):
    line = line.rstrip("\n")
    if line.startswith("BEGIN ") and current is None:
        current = [line[len("BEGIN "):], []]
    elif line == "END" and current is not None:
        events.append((current[0], current[1]))
        current = None
    elif current is not None:
        current[1].append(line)
scp = [args for kind, args in events if kind == "scp"]
ssh = [args for kind, args in events if kind == "ssh"]
targets = [args[-1] for args in scp]
if len(targets) != 1 or not targets[0].endswith(":~/.local/bin/agent-desktop-macos-helper.new"):
    sys.exit("an old guest with a matching CLI did not repair only the missing helper: %r" % scp)
if any("agent-desktop.new" in args for args in scp):
    sys.exit("an old guest with a matching CLI copied agent-desktop again")
last_scp = max(index for index, (kind, _) in enumerate(events) if kind == "scp")
last_ssh = max(index for index, (kind, _) in enumerate(events) if kind == "ssh")
if last_scp > last_ssh:
    sys.exit("an old guest ran the command before staging the missing helper")
PY
FAKE_GUEST_SHA1=2222222222222222222222222222222222222222
FAKE_GUEST_HELPER_SHA1=3333333333333333333333333333333333333333
export FAKE_GUEST_SHA1 FAKE_GUEST_HELPER_SHA1

kill "$vm_pid" 2>/dev/null || true
wait "$vm_pid" 2>/dev/null || true
vm_pid=

sleep 30 &
vm_pid=$!
printf '%s\n' "$vm_pid" > "$AD_SANDBOX_DIR/vm/vmhost.pid"
FAKE_PS_COMMAND="/opt/agent-desktop/build/vmhost run $AD_SANDBOX_DIR/vm"
rm -f "$AD_SANDBOX_DIR/setup_done"
probe_started=$(date +%s)
run_ok_with FAKE_SSH_BEHAVIOR silent status demo
probe_elapsed=$(( $(date +%s) - probe_started ))
assert_field "$tmp/out" state needs_setup 'status against a connected but silent guest'
[ "$probe_elapsed" -lt 25 ] || \
  fail "status blocked ${probe_elapsed}s on a silent guest instead of reporting a state inside the 30s status budget"

probe_started=$(date +%s)
run_ok_with FAKE_SSH_BEHAVIOR unreachable status demo
probe_elapsed=$(( $(date +%s) - probe_started ))
assert_field "$tmp/out" state needs_setup 'status against an unreachable guest'
[ "$probe_elapsed" -lt 25 ] || \
  fail "status blocked ${probe_elapsed}s on an unreachable guest instead of using a bounded connect timeout"

run_ok_with FAKE_SSH_BEHAVIOR granted status demo
assert_field "$tmp/out" state running 'status against a guest with Accessibility granted'
[ -f "$AD_SANDBOX_DIR/setup_done" ] || fail 'a completed guest setup was not recorded'
touch -t 202001010000 "$AD_SANDBOX_DIR/setup_done"
setup_time=$(stat -f %m "$AD_SANDBOX_DIR/setup_done")
run_ok_with FAKE_SSH_BEHAVIOR granted status demo
[ "$(stat -f %m "$AD_SANDBOX_DIR/setup_done")" = "$setup_time" ] || \
  fail 'a later status probe moved the setup time that orders automatic clone sources'

: > "$AD_SANDBOX_DIR/setup_done"
FAKE_SCP_BEHAVIOR=prompt
export FAKE_SCP_BEHAVIOR
run_ok_with FAKE_SSH_BEHAVIOR quiet exec demo -- snapshot
unset FAKE_SCP_BEHAVIOR
grep -qx 'guest output' "$tmp/out" || \
  fail "exec did not carry a quiet remote command past the status probe bound: $(cat "$tmp/out" "$tmp/err")"

status=0
run_provider_with VMHOST "$tmp/no-vmhost" status demo > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "status without the VM host binary exited $status: $(cat "$tmp/out" "$tmp/err")"
assert_field "$tmp/out" code ACTION_FAILED 'status without the VM host binary'
status=0
run_provider_with VMHOST "$tmp/no-vmhost" exec demo -- snapshot > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 125 ] || fail "exec without the VM host binary exited $status: $(cat "$tmp/out" "$tmp/err")"
grep -q 'The macOS VM host is not installed\.' "$tmp/out" || fail "exec without the VM host binary did not say so: $(cat "$tmp/out")"

kill "$vm_pid" 2>/dev/null || true
wait "$vm_pid" 2>/dev/null || true
vm_pid=
FAKE_PS_COMMAND=
rm -f "$AD_SANDBOX_DIR/setup_done"

printf '999999999\n' > "$AD_SANDBOX_DIR/vm/vmhost.pid"
status=0
run_provider exec demo -- snapshot > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 125 ] || fail "exec on a stopped sandbox exited $status"
grep -q 'Sandbox demo is stopped\.' "$tmp/out" || fail 'exec on a stopped sandbox did not report it'

printf 'provider tests passed\n'
