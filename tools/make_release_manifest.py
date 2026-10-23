import hashlib
import json
import datetime
import os
import sys

version = sys.argv[1] if len(sys.argv) > 1 else '1.1.1'
base = f'release/{version}'


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


exe = os.path.join(base, 'AimyriaOne-x64.exe')
dll = os.path.join(base, 'WebView2Loader.dll')

manifest = {
    "version": version,
    "url": "https://aimyria.com/releases/latest/download/AimyriaOne-x64.exe",
    "sha256": sha256(exe),
    "notes": "密钥显示分组与名称，更好选择",
    "pubDate": datetime.datetime.now(
        datetime.timezone(datetime.timedelta(hours=8))
    ).replace(microsecond=0).isoformat(),
    "size": os.path.getsize(exe),
    "companions": [{
        "name": "WebView2Loader.dll",
        "sha256": sha256(dll),
        "url": "https://aimyria.com/releases/latest/download/WebView2Loader.dll",
    }],
}

with open(os.path.join(base, 'version.json'), 'w', encoding='utf-8') as f:
    json.dump(manifest, f, ensure_ascii=False, indent=2)
    f.write('\n')

with open(os.path.join(base, 'AimyriaOne-x64.exe.sha256'), 'w') as f:
    f.write(f"{manifest['sha256']}  AimyriaOne-x64.exe\n")

print('version:', manifest['version'])
print('sha256 :', manifest['sha256'])
print('size   :', manifest['size'])
