import os
import sys
import json
import re

imports_block = """from __future__ import annotations
import json
import os
import queue
import random
import re
import http.cookiejar
import sys
import threading
import time
import xml.etree.ElementTree as ET
import zipfile
import shutil
import tempfile
import urllib.request
import urllib.parse
from urllib.request import Request
from dataclasses import asdict, dataclass, field, fields
from datetime import datetime, timezone
from importlib import import_module
from pathlib import Path
from urllib.parse import parse_qs, parse_qsl, unquote, urlencode, urljoin, urlparse
import wx
import wx.adv
try:
    import winreg
except ImportError:
    pass
try:
    import ctypes
except ImportError:
    pass

from apricot.constants import *
from apricot.locales import TEXT
"""

def extract_methods_to_mixin(source_file, target_file, mixin_name, methods):
    with open(source_file, 'r', encoding='utf-8') as f:
        lines = f.readlines()
        
    extracted = []
    new_lines = []
    
    in_method = False
    current_method = None
    method_buffer = []
    
    i = 0
    while i < len(lines):
        line = lines[i]
        
        # Look for def or decorators
        is_target = False
        method_name_found = None
        j = i
        
        while j < len(lines):
            l = lines[j]
            if l.startswith('    def '):
                match = re.match(r'    def ([a-zA-Z0-9_]+)\(', l)
                if match:
                    name = match.group(1)
                    if name in methods:
                        is_target = True
                        method_name_found = name
                break
            elif l.startswith('    @'):
                j += 1
            elif l.strip() == '':
                j += 1
            else:
                break
                
        if is_target:
            method_code = []
            # Extract everything from i (which is decorators) down to the end of the method
            k = j + 1
            while k < len(lines):
                l2 = lines[k]
                if (l2.startswith('    def ') or l2.startswith('    @') or l2.startswith('    class ')) and l2.strip():
                    break
                if l2.strip() and not l2.startswith(' ') and not l2.startswith('\t'):
                    break
                k += 1
            
            for index in range(i, k):
                method_code.append(lines[index])
            
            extracted.append("".join(method_code))
            methods.remove(method_name_found)
            i = k
            continue
        
        new_lines.append(line)
        i += 1
        
    if not extracted:
        print(f"No methods found to extract for {mixin_name}.")
        if methods:
            print(f"Failed to find: {methods}")
        return False
        
    os.makedirs(os.path.dirname(target_file), exist_ok=True)
    
    mode = 'a' if os.path.exists(target_file) else 'w'
    with open(target_file, mode, encoding='utf-8') as f:
        if mode == 'w':
            f.write(imports_block + "\n")
            f.write(f"class {mixin_name}:\n")
        else:
            f.write("\n")
            
        for chunk in extracted:
            f.write(chunk)
            f.write("\n")
            
    # Modify inheritance in wx_main.py
    for i, line in enumerate(new_lines):
        if 'class MainFrame(' in line:
            if mixin_name not in line:
                new_lines[i] = line.replace('class MainFrame(', f'class MainFrame({mixin_name}, ')
            break
            
    module_path = target_file.replace('.py', '').replace('/', '.').replace('\\', '.')
    import_stmt = f"from {module_path} import {mixin_name}\n"
    
    new_lines.insert(2, import_stmt)
    
    with open(source_file, 'w', encoding='utf-8') as f:
        f.writelines(new_lines)
        
    print(f"Successfully extracted {len(extracted)} methods to {mixin_name} in {target_file}")
    if methods:
        print(f"WARNING: The following methods were NOT found: {methods}")
    return True

if __name__ == '__main__':
    args = sys.argv[1:]
    if len(args) < 3:
        sys.exit(1)
    target_file = args[0]
    mixin_name = args[1]
    methods_to_extract = set(args[2:])
    extract_methods_to_mixin('wx_main.py', target_file, mixin_name, methods_to_extract)
