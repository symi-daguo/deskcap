#!/usr/bin/env bash
# Build DeskCapIME.app — embeds librime; prefers Developer ID + hardened runtime for distribution.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Prefer local disk for codesign/dylib rewrite; NAS can deny overwrites mid-build.
# 0.6.69 — 构建目录**不能放 /tmp**。
# 教训：LaunchServices 会把 /tmp/DeskCapIME-build/DeskCapIME.app 也注册成一个
# 输入法候选；IMK 拉起时 Bundle.main.resourcePath 解析到 /tmp 的副本，
# 而 /tmp 里的 app 在装机后被清空 → rime 读不到完整资源 → 切过来打不出字，
# 且日志只显示 server up（2026-10-20 实际踩到）。
OUT_ROOT="$ROOT/.build-ime"
mkdir -p "$OUT_ROOT"
OUT="$OUT_ROOT/DeskCapIME.app"
BIN="$OUT/Contents/MacOS/DeskCapIME"
FW="$OUT/Contents/Frameworks"
SRC="$ROOT/Sources/DeskCapIME"
RIME_PREFIX="${RIME_PREFIX:-/opt/homebrew}"
BUNDLE_ID="com.office.deskcap.inputmethod.symi"

if [[ ! -f "$RIME_PREFIX/include/rime_api.h" ]]; then
  RIME_PREFIX=/usr/local
fi
if [[ ! -f "$RIME_PREFIX/include/rime_api.h" ]]; then
  echo "librime headers not found. brew install librime" >&2
  exit 1
fi

pick_identity() {
  if [[ -n "${DESKCAP_SIGN_IDENTITY:-}" ]]; then
    printf '%s\n' "$DESKCAP_SIGN_IDENTITY"
    return
  fi
  local id
  id=$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' | head -1 || true)
  if [[ -n "$id" ]]; then
    printf '%s\n' "$id"
    return
  fi
  security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/.*"\(Apple Development:[^"]*\)".*/\1/p' | head -1 || true
}

IDENTITY="$(pick_identity)"
USE_HARDENED=0
case "$IDENTITY" in
  "Developer ID Application:"*) USE_HARDENED=1 ;;
esac

# Menu-bar icons must be template-safe TIFF (not solid PNG tiles).
#
# 2026-10-20 — Pillow 是可选项：图标产物（Resources/app_logo*.png +
# menubar TIFF）已提交，日常构建不需要重新生成。CI runner 的 python3
# 没有 Pillow，硬跑会 ModuleNotFoundError 把整个构建打挂（首跑实测）。
# 只有真的改了图标时才需要 `pip install Pillow` 后手动跑 gen-icons.py。
# 只有 gen-icons.py 比产物新时才重新生成。不同 Pillow 版本的 zlib 压缩参数
# 不同 → 每次构建都会产出"像素相同、字节不同"的 PNG，把仓库 diff 弄脏
# （2026-10-20 两次踩到）。产物已提交，日常构建不必重生成。
if [ -f "$ROOT/Resources/app_logo.png" ] && [ ! "$ROOT/scripts/gen-icons.py" -nt "$ROOT/Resources/app_logo.png" ]; then
  echo "icons: 产物已是最新，跳过生成"
elif python3 -c 'import PIL' 2>/dev/null; then
  python3 "$ROOT/scripts/gen-icons.py"
else
  echo "WARN: Pillow 缺失，跳过图标生成（使用已提交的 Resources/app_logo*.png）" >&2
fi

rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources/rime-data" "$FW"
cp "$ROOT/Info.plist" "$OUT/Contents/Info.plist"
cp -R "$ROOT/rime-data/"* "$OUT/Contents/Resources/rime-data/"
cp "$ROOT/Resources/AppIcon.icns" "$OUT/Contents/Resources/AppIcon.icns"
cp "$ROOT/Resources/app_logo.png" "$OUT/Contents/Resources/app_logo.png"
cp "$ROOT/Resources/app_logo.tiff" "$OUT/Contents/Resources/app_logo.tiff"
for s in 16 18 32 64 128; do
  cp "$ROOT/Resources/app_logo_${s}.png" "$OUT/Contents/Resources/app_logo_${s}.png"
done
printf 'APPL????' > "$OUT/Contents/PkgInfo"

mkdir -p "$OUT/Contents/Resources/zh-Hans.lproj" "$OUT/Contents/Resources/en.lproj"
cat > "$OUT/Contents/Resources/zh-Hans.lproj/InfoPlist.strings" <<'STR'
CFBundleDisplayName = "DeskCap拼音";
CFBundleName = "DeskCap拼音";
"com.office.deskcap.inputmethod.symi.pinyin" = "DeskCap拼音";
STR
cat > "$OUT/Contents/Resources/en.lproj/InfoPlist.strings" <<'STR'
CFBundleDisplayName = "DeskCap Pinyin";
CFBundleName = "DeskCap Pinyin";
"com.office.deskcap.inputmethod.symi.pinyin" = "DeskCap Pinyin";
STR

# Copy + rename Homebrew dylibs into Frameworks (stable sonames)
python3 - <<PY
import os, shutil, subprocess
rime_prefix = "$RIME_PREFIX"
fw = "$FW"

def deps(path):
    out = subprocess.check_output(["otool", "-L", path], text=True)
    libs = []
    for line in out.splitlines()[1:]:
        p = line.strip().split(" ")[0]
        if p.startswith("/opt/homebrew") or p.startswith("/usr/local"):
            libs.append(p)
    return libs

seen = set()
q = [f"{rime_prefix}/lib/librime.1.dylib"]
while q:
    p = q.pop()
    if p in seen:
        continue
    seen.add(p)
    if os.path.exists(p):
        for d in deps(p):
            if d not in seen:
                q.append(d)

def soname(path):
    base = os.path.basename(os.path.realpath(path))
    if base.startswith("librime"):
        return "librime.1.dylib"
    if base.startswith("libglog"):
        return "libglog.2.dylib"
    if base.startswith("libyaml-cpp"):
        return "libyaml-cpp.0.9.dylib"
    if base.startswith("libgflags"):
        return "libgflags.2.3.dylib"
    if base.startswith("libleveldb"):
        return "libleveldb.1.dylib"
    if base.startswith("libmarisa"):
        return "libmarisa.0.dylib"
    if base.startswith("libopencc"):
        return "libopencc.1.4.dylib"
    if base.startswith("libsnappy"):
        return "libsnappy.1.dylib"
    return base

copied = {}
for p in sorted(seen):
    if not os.path.exists(p):
        raise SystemExit(f"missing {p}")
    name = soname(p)
    if name in copied.values():
        print("skip dup", p, "->", name)
        continue
    dst = os.path.join(fw, name)
    shutil.copy2(os.path.realpath(p), dst)
    os.chmod(dst, 0o755)
    copied[p] = name
    print("embed", p, "->", name)

# rewrite install names
for name in set(copied.values()):
    lib = os.path.join(fw, name)
    subprocess.run(["install_name_tool", "-id", f"@rpath/{name}", lib], check=False)
    out = subprocess.check_output(["otool", "-L", lib], text=True)
    for line in out.splitlines()[1:]:
        dep = line.strip().split(" ")[0]
        if not (dep.startswith("/opt/homebrew") or dep.startswith("/usr/local")):
            continue
        new = f"@rpath/{soname(dep)}"
        subprocess.run(["install_name_tool", "-change", dep, new, lib], check=False)
print("embedded", len(set(copied.values())), "dylibs")
PY

# Linker expects -lrime → librime.dylib
ln -sf librime.1.dylib "$FW/librime.dylib"

swiftc -O -parse-as-library \
  -framework Cocoa -framework InputMethodKit \
  -I"$RIME_PREFIX/include" \
  -L"$FW" -lrime \
  -import-objc-header "$SRC/RimeBridge.h" \
  -Xlinker -rpath -Xlinker "@executable_path/../Frameworks" \
  -o "$BIN" "$SRC"/*.swift
# 0.6.62 — 上面用 glob 而非硬编码列表：加 IMELog.swift 时硬编码列表漏编，
# 报了 "cannot find 'IMELog' in scope"。以后加源文件不必再改这行。

# Ensure binary loads embedded librime
otool -L "$BIN" | awk '/librime/{print $1}' | while read -r dep; do
  [[ "$dep" == @rpath/* ]] && continue
  install_name_tool -change "$dep" "@rpath/librime.1.dylib" "$BIN" || true
done

chmod +x "$BIN"

USER_RIME="$HOME/Library/Application Support/DeskCap/rime"
mkdir -p "$USER_RIME"
# rime schema/dicts → 用户目录（rime 靠 _schema_version 管版本，直接覆盖即可）。
# 译词表 glossary.*.tsv **排除在外**：它是用户可自行扩充的数据，被种子覆盖等于
# 每次重新打包都清空用户词表。缺失时才种子。
find "$ROOT/rime-data" -mindepth 1 -maxdepth 1 ! -name 'glossary.*.tsv' \
  -exec cp -R {} "$USER_RIME/" \;
for g in "$ROOT/rime-data"/glossary.*.tsv; do
  [[ -e "$g" ]] || continue
  dest="$USER_RIME/$(basename "$g")"
  if [[ -e "$dest" ]]; then
    echo "keep  user glossary: $(basename "$g")"
  else
    cp "$g" "$dest"
    echo "seed  glossary: $(basename "$g")"
  fi
done
rm -f "$USER_RIME/.deskcap_seeded"

sign_one() {
  local target="$1"
  if [[ -z "$IDENTITY" ]]; then
    codesign --force --sign - "$target"
    return
  fi
  if [[ "$USE_HARDENED" == 1 ]]; then
    codesign --force --options runtime --timestamp --sign "$IDENTITY" "$target"
  else
    codesign --force --sign "$IDENTITY" "$target"
  fi
}

for lib in "$FW"/*.dylib; do
  sign_one "$lib"
done
sign_one "$OUT"

echo "Signed: ${IDENTITY:-ad-hoc} hardened=$USE_HARDENED ($BUNDLE_ID)"
codesign --verify --deep --strict "$OUT"

pkill -x DeskCapIME 2>/dev/null || true
# Connection name is owned by the *installed* IME — smoke may log "could not register".
# Only fail on hard load/crash errors.
"$OUT/Contents/MacOS/DeskCapIME" >/tmp/deskcapime-smoke.log 2>&1 &
SMOKE_PID=$!
sleep 1
if ! kill -0 "$SMOKE_PID" 2>/dev/null; then
  echo "ERROR: DeskCapIME crashed on launch. Log:" >&2
  cat /tmp/deskcapime-smoke.log >&2 || true
  exit 1
fi
if grep -q 'Library not loaded\|dyld\|Symbol not found' /tmp/deskcapime-smoke.log; then
  echo "ERROR: dynamic library load failed:" >&2
  cat /tmp/deskcapime-smoke.log >&2
  kill "$SMOKE_PID" 2>/dev/null || true
  exit 1
fi
kill "$SMOKE_PID" 2>/dev/null || true
wait "$SMOKE_PID" 2>/dev/null || true

# Do NOT install into ~/Library — dual install creates duplicate broken entries in Settings.
# Artifact only; install to /Library after notarize.
pkill -x DeskCapIME 2>/dev/null || true
mkdir -p "$ROOT/build"
rm -rf "$ROOT/build/DeskCapIME.app"
ditto "$OUT" "$ROOT/build/DeskCapIME.app"

echo ""
echo "Built: $OUT"
echo "Artifact: $ROOT/build/DeskCapIME.app  （改名后：$ROOT/build/DeskCapIME.app.built）"
if [[ "$USE_HARDENED" == 1 ]]; then
  echo ""
  echo "=== 清理旧包 + 公证 + 只装一份到 /Library ==="
  echo "  bash $ROOT/scripts/notarize-ime.sh"
  echo "  sudo rm -rf \"/Library/Input Methods/DeskCapIME.app\""
  echo "  sudo ditto \"$ROOT/build/DeskCapIME.app\" \"/Library/Input Methods/DeskCapIME.app\""
  echo "  rm -rf \"\$HOME/Library/Input Methods/DeskCapIME.app\""
  echo "然后注销登录，添加里应只剩 1 个 DeskCap拼音"
  echo ""
  # 0.6.55 — 装机中断提示。更新 IME 必须重启 IME 进程（pkill -x DeskCapIME），
  # 若此刻用户正在用 DeskCap拼音打字，输入会话会当场中断、表现为「打字打不出来」。
  # 系统通常会在下次击键时自动拉起新进程，但用户需要重新切一次输入法才恢复。
  # 2026-10-14 就踩了这个坑——装机前先看看有没有在用。
  # 0.6.70 — 构建产物绝不留 .app 后缀。
#
# LaunchServices 会把**任何** .app 目录注册成可执行包。构建目录里的
# DeskCapIME.app 一旦被注册，IMK 可能拉起那一份（Bundle.main.resourcePath
# 指向构建目录），而构建目录随时会被清理 → rime-data 缺失 → 切过来打不出字。
# 2026-10-20 这个坑连踩两次（/tmp 一次、.build-ime 一次）。
# 装机后立即改名，LS 不再识别；下次构建会重新生成。
# ── 顺序至关重要：先装到系统，再改名产物 ──
#
# 2026-10-21 踩坑：改名放在最前面 → 装机脚本找不到源（.app.built）→
# ditto 报 "Operation not permitted"（外置卷上带 .built 后缀的目录不被
# ditto 接受）→ /Library 里的旧包被移走后**输入法直接消失**。
#
# 现在：① 提权安装（INSTALL_TO_SYSTEM=1 时，CI 不设该变量）→ ② 改名产物。
# 改名仍要做：任何 .app 目录都会被 LaunchServices 注册成输入法候选，
# IMK 可能拉起构建目录里那份（资源不完整 → 打不出字）。
if [[ "${INSTALL_TO_SYSTEM:-0}" == "1" ]]; then
  echo ""
  echo "=== 安装到 /Library/Input Methods（需要管理员授权）==="
  APP_SRC="$ROOT/build/DeskCapIME.app"
  if [ -d "$APP_SRC" ]; then
    osascript -e "do shell script \"rm -rf '/Library/Input Methods/DeskCapIME.app' && ditto '$APP_SRC' '/Library/Input Methods/DeskCapIME.app'\" with administrator privileges" 2>&1 | tail -2
    if codesign --verify --deep --strict "/Library/Input Methods/DeskCapIME.app" 2>/dev/null; then
      echo "已安装：$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' '/Library/Input Methods/DeskCapIME.app/Contents/Info.plist')"
    else
      echo "⚠️ 安装后签名校验失败，请手动检查"
    fi

    # 装机完成后才改名产物（顺序不能反：改名后 ditto 不接受该目录）。
    # 反注册必须在改名**之前**（改名后路径不存在，-u 找不到）。
    # 只改名不反注册的话，LS 会留下指向「已不存在路径」的记录，IMK 仍可能去拉它。
    LSR="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
    for stale in "$OUT" "$ROOT/build/DeskCapIME.app"; do
      if [ -d "$stale" ]; then
        "$LSR" -u "$stale" 2>/dev/null || true
        mv "$stale" "$stale.built" 2>/dev/null || true
      fi
    done
    "$LSR" -f "/Library/Input Methods/DeskCapIME.app" 2>/dev/null || true
    echo "构建产物已改名为 *.app.built 并反注册（防止 LaunchServices 注册）"
  fi
fi

# 反注册必须在改名**之前**（改名后路径就不存在了，-u 找不到）。
# LaunchServices 会扫到构建过程中短暂存在的 .app；只改名不反注册的话，
# LS 里会留下一条指向「已不存在路径」的记录，IMK 仍可能去拉它。
  echo "（CI 不装机，保留 .app 产物供 Verify 步骤检查）"

echo ""echo "⚠️  装机前请确认：若正在用「DeskCap拼音」打字，先切到别的输入法再装，"
  echo "    否则装完需要重新切换一次输入法才能恢复输入。"
else
  echo "WARNING: 非 Developer ID"
fi
