import sys
import json
import os
sys.path.append('.')
from locales import TEXT

os.makedirs('apricot/locales', exist_ok=True)
for lang, strings in TEXT.items():
    with open(f'apricot/locales/{lang}.json', 'w', encoding='utf-8') as f:
        json.dump(strings, f, ensure_ascii=False, indent=4)

with open('apricot/locales/__init__.py', 'w', encoding='utf-8') as f:
    f.write('''import json
from pathlib import Path
import os
import sys

def _get_base_path():
    if getattr(sys, "frozen", False) and hasattr(sys, "_MEIPASS"):
        return Path(sys._MEIPASS)
    return Path(__file__).resolve().parent.parent.parent

def load_locales():
    locales_dir = _get_base_path() / "apricot" / "locales"
    if not locales_dir.exists():
        locales_dir = Path(__file__).parent
        
    TEXT = {}
    for json_file in locales_dir.glob("*.json"):
        lang = json_file.stem
        try:
            with open(json_file, "r", encoding="utf-8") as f:
                TEXT[lang] = json.load(f)
        except Exception as e:
            print(f"Error loading {json_file}: {e}")
    return TEXT

TEXT = load_locales()
''')
