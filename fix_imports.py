import sys
from pathlib import Path

for p in Path('apricot/ui').glob('*.py'):
    content = p.read_text('utf-8')
    target = '__import__("wx_main").MainFrame.'
    if target in content:
        new_content = content.replace(target, 'MiscUI.')
        if 'class MiscUI' not in new_content and 'from apricot.ui.misc import MiscUI' not in new_content:
            lines = new_content.splitlines()
            last_import = 0
            for i, line in enumerate(lines):
                if line.startswith('import ') or line.startswith('from '):
                    last_import = i
            lines.insert(last_import + 1, 'from apricot.ui.misc import MiscUI')
            new_content = '\n'.join(lines) + '\n'
            
        p.write_text(new_content, 'utf-8')
        print(f'Fixed {p}')
