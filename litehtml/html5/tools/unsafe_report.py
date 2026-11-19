import json, os, re, subprocess, sys

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
meta = json.loads(subprocess.check_output(['cargo', 'metadata', '--format-version', '1', '--manifest-path', os.path.join(root, 'Cargo.toml')]))
failed = False
for package in meta['packages']:
    base = os.path.dirname(package['manifest_path'])
    if not base.startswith(root):
        print('external crate in the build graph:', package['name'], package['version'])
        failed = True
        continue
    count = 0
    for dirpath, _, files in os.walk(base):
        if '/target' in dirpath or '/tools/' in dirpath + '/' or dirpath.endswith('/tests'):
            continue
        for name in files:
            if name.endswith('.rs') and name != 'rules.in.rs':
                text = open(os.path.join(dirpath, name), encoding='utf-8').read()
                count += len(re.findall(r'\bunsafe\b', text))
    lib = [t for t in package['targets'] if 'lib' in t['kind'] or 'staticlib' in t['kind']]
    forbids = any('#![forbid(unsafe_code)]' in open(t['src_path'], encoding='utf-8').read() for t in lib)
    rel = os.path.relpath(base, root)
    print(f'{package["name"]:16} {rel:24} unsafe {count:3}  forbid(unsafe_code) {"yes" if forbids else "no"}')
    if package['name'] != 'lh-html5-ffi' and (count or not forbids):
        failed = True
sys.exit(1 if failed else 0)
