"""Decode every generated photo artifact and check its recorded dimensions."""
import json
import sqlite3
import sys
from pathlib import Path
from PIL import Image

root = Path(sys.argv[1]).resolve()
output = Path(sys.argv[2]).resolve()
db = sqlite3.connect((root / '.cullant/cullant.db').as_uri() + '?mode=ro', uri=True)
db.row_factory = sqlite3.Row
rows = db.execute('''SELECT t.*, f.orientation AS current_orientation
    FROM thumbnails t JOIN files f ON f.id=t.file_id
    WHERE f.status=0 AND f.kind IN (0,1) AND t.kind IN (0,1)''').fetchall()
failures = []
for row in rows:
    path = root / '.cullant/thumbs' / row['cache_path']
    try:
        assert not row['failed'], 'Decoder failure row'
        assert path.stem.split('_')[2] == str(row['current_orientation'] or 1), 'Stale orientation key'
        with Image.open(path) as image:
            image.load()
            assert image.size == (row['width'], row['height']), 'Recorded dimensions differ'
            assert max(image.size) <= (384 if row['kind'] == 0 else 2560), 'Unexpected final size'
    except Exception as error:
        failures.append({'fileId': row['file_id'], 'kind': row['kind'], 'error': str(error)})
result = {'artifacts': len(rows), 'failures': failures, 'status': 'FAIL' if failures else 'PASS'}
output.write_text(json.dumps(result, indent=2))
print(json.dumps(result))
sys.exit(bool(failures))
