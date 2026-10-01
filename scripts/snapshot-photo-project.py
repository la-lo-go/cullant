"""Read source hashes and existing subproject cache metadata. Never write media."""
import hashlib
import json
import os
import sys
from pathlib import Path

root = Path(sys.argv[1]).resolve()
output = Path(sys.argv[2]).resolve()
rows = []
caches = []
for directory, dirs, files in os.walk(root):
    dirs[:] = [d for d in dirs if d not in ('System Volume Information', '$RECYCLE.BIN', '_trash')
               and (not d.startswith('.') or d == '.cullant')]
    if Path(directory) == root:
        dirs[:] = [d for d in dirs if d != '.cullant']
    for name in files:
        path = Path(directory) / name
        info = path.stat()
        row = {'path': str(path), 'bytes': info.st_size, 'mtimeNs': info.st_mtime_ns}
        if '.cullant' in path.relative_to(root).parts:
            caches.append(row)
            continue
        if path.suffix.lower() in ('.raf', '.jpg', '.jpeg', '.png', '.xmp'):
            digest = hashlib.sha256()
            with path.open('rb') as source:
                for block in iter(lambda: source.read(4 * 1024 * 1024), b''):
                    digest.update(block)
            row['sha256'] = digest.hexdigest()
        rows.append(row)
output.write_text(json.dumps({'sources': rows, 'existingCaches': caches}, indent=2))
print(json.dumps({'sourceFiles': len(rows), 'hashedPhotosAndSidecars': sum('sha256' in r for r in rows),
                  'existingCacheFiles': len(caches)}))
