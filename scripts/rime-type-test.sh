#!/usr/bin/env bash
# Standard librime typing test for DeskCapIME schema.
# Usage: bash scripts/rime-type-test.sh [rime_data_dir]
# Exit 0 only if all cases pass (simplified + candidates + space commit).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="${1:-$ROOT/macos-ime/rime-data}"
PROBE_SRC="${TMPDIR:-/tmp}/deskcap-rime-probe.c"
PROBE_BIN="${TMPDIR:-/tmp}/deskcap-rime-probe"

cat > "$PROBE_SRC" <<'EOF'
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <rime_api.h>
int main(int argc, char** argv) {
  const char* dir = argc>1?argv[1]:".";
  const char* keys = argc>2?argv[2]:"zhongwen";
  if (chdir(dir) != 0) { perror("chdir"); return 2; }
  RimeApi* api = rime_get_api();
  RimeTraits t; memset(&t,0,sizeof(t));
  t.data_size = (int)(sizeof(RimeTraits) - sizeof(int));
  t.shared_data_dir = ".";
  t.user_data_dir = ".";
  t.distribution_name = "DeskCapProbe";
  t.distribution_code_name = "deskcap";
  t.distribution_version = "0.5";
  t.app_name = "rime.deskcap.probe";
  api->setup(&t);
  api->initialize(&t);
  api->start_maintenance(True);
  api->join_maintenance_thread();
  RimeSessionId s = api->create_session();
  if(!s){ fprintf(stderr,"NO_SESSION\n"); return 1; }
  for(const char*p=keys;*p;p++) api->process_key(s,(int)(unsigned char)*p,0);
  RimeContext ctx; memset(&ctx,0,sizeof(ctx));
  ctx.data_size = (int)(sizeof(ctx)-sizeof(int));
  if(api->get_context(s,&ctx)){
    printf("PREEDIT=%s\n", ctx.composition.preedit?ctx.composition.preedit:"");
    printf("N=%d\n", ctx.menu.num_candidates);
    for(int i=0;i<ctx.menu.num_candidates && i<8;i++)
      printf("CAND=%s\n", ctx.menu.candidates[i].text?ctx.menu.candidates[i].text:"");
    api->free_context(&ctx);
  }
  api->process_key(s, 0x20, 0);
  RimeCommit commit; memset(&commit,0,sizeof(commit));
  commit.data_size = (int)(sizeof(commit)-sizeof(int));
  if(api->get_commit(s,&commit)){
    printf("COMMIT=%s\n", commit.text?commit.text:"");
    api->free_commit(&commit);
  } else printf("COMMIT=\n");
  api->destroy_session(s);
  api->finalize();
  return 0;
}
EOF

cc -O2 "$PROBE_SRC" -I/opt/homebrew/include -L/opt/homebrew/lib -lrime -o "$PROBE_BIN"
export GLOG_minloglevel=2
WORK="$(mktemp -d)"
cp -R "$DIR"/. "$WORK"/
# Always test current sources, not stale build/
rm -rf "$WORK/build"

run() { "$PROBE_BIN" "$WORK" "$1" 2>/dev/null; }

fail=0
check_case() {
  local keys="$1" expect_commit="$2" forbid=""
  shift 2
  # remaining args are forbidden strings
  forbid=("$@")
  local out commit
  out="$(run "$keys")"
  commit="$(echo "$out" | sed -n 's/^COMMIT=//p')"
  echo "-- $keys => COMMIT=[$commit]"
  echo "$out" | sed -n 's/^CAND=/  cand /p' | head -6
  if [[ "$commit" != "$expect_commit" ]]; then
    echo "FAIL commit expected=[$expect_commit] got=[$commit]"
    fail=1
  fi
  local n
  n="$(echo "$out" | sed -n 's/^N=//p')"
  if [[ -z "$n" || "$n" -lt 1 ]]; then
    echo "FAIL no candidates"
    fail=1
  fi
  for b in "${forbid[@]:-}"; do
    [[ -z "$b" ]] && continue
    if echo "$out" | grep -q "$b"; then
      echo "FAIL forbidden token present: $b"
      fail=1
    fi
  done
}

# Simplified-first standard phrases (forbid common traditional forms)
check_case "zhongwen" "中文" "種" "國" "體"
check_case "ruanjian" "软件" "軟件" "軟體"
check_case "jianpan" "键盘" "鍵盤"
check_case "shurufa" "输入法" "輸入法"
check_case "ceshi" "测试" "測試"
check_case "xianzai" "现在" "現在"

# Digit selection: type pinyin, press '2' for second candidate then space? 
# Probe only does space; digit path tested in Controller via process_key.
# Extra: long phrase still produces commit
check_case "nihao" "你好"

rm -rf "$WORK"
if [[ "$fail" -ne 0 ]]; then
  echo "RIME_TYPE_TEST FAILED"
  exit 1
fi
echo "RIME_TYPE_TEST OK"
