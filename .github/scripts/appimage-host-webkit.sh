#!/usr/bin/env bash
# Repacks AppImages so they run on the system WebKitGTK when it is installed and keep the bundled copy as a fallback.
set -euo pipefail

appimagetool_version=1.9.1
appimagetool_sha256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0

if [ "$#" -eq 0 ]; then
	echo "usage: $0 <AppImage>..." >&2
	exit 1
fi

export APPIMAGE_EXTRACT_AND_RUN=1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

appimagetool="$work/appimagetool"
curl -fsSL -o "$appimagetool" "https://github.com/AppImage/appimagetool/releases/download/$appimagetool_version/appimagetool-x86_64.AppImage"
echo "$appimagetool_sha256  $appimagetool" | sha256sum -c -
chmod +x "$appimagetool"

for appimage in "$@"; do
	appimage=$(readlink -f "$appimage")
	appdir="$work/AppDir"
	rm -rf "$appdir" "$work/squashfs-root"
	(cd "$work" && "$appimage" --appimage-extract > /dev/null)
	mv "$work/squashfs-root" "$appdir"

	binary=$(sed -n 's/^Exec=\([^ ]*\).*/\1/p' "$appdir"/*.desktop | head -n 1)
	test -x "$appdir/usr/bin/$binary"

	python3 - "$appdir/AppRun" "$binary" <<-'PY'
		import sys
		path, binary = sys.argv[1], sys.argv[2]
		lines = open(path).read().split("\n")
		anchor = next(i for i, line in enumerate(lines) if line.startswith('this_dir='))
		assert any(line.startswith('exec ') for line in lines), "AppRun has no exec line"
		launcher = f'''
		if [ -z "${{NWMPUBLISHER_BUNDLED_WEBKIT:-}}" ]; then
		    host_webkit=$(PATH="/usr/sbin:/sbin:$PATH" ldconfig -p 2>/dev/null | awk '/libwebkit2gtk-4\\.1\\.so\\.0 .*x86-64/ {{ print $NF; exit }}' || true)
		    if [ -n "$host_webkit" ] && [ -e "$host_webkit" ]; then
		        export LD_LIBRARY_PATH="$(dirname "$host_webkit")${{LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}}"
		        exec "$this_dir"/usr/bin/{binary} "$@"
		    fi
		fi'''
		lines[anchor + 1:anchor + 1] = launcher.strip("\n").split("\n")
		open(path, "w").write("\n".join(lines))
	PY

	offset=$("$appimage" --appimage-offset)
	head -c "$offset" "$appimage" > "$work/runtime"
	ARCH=x86_64 "$appimagetool" --comp zstd --runtime-file "$work/runtime" "$appdir" "$work/out.AppImage"
	mv "$work/out.AppImage" "$appimage"
	chmod +x "$appimage"
	echo "Patched $appimage"
done
