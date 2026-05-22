import os
import glob

imports = """from __future__ import annotations
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

for root, _, files in os.walk('apricot'):
    for file in files:
        if file.endswith('.py') and file not in ['constants.py', 'settings.py', 'locales.py', '__init__.py']:
            path = os.path.join(root, file)
            with open(path, 'r', encoding='utf-8') as f:
                content = f.read()
            
            # Remove any existing import wx
            content = content.replace('import wx\n', '')
            
            # Add new imports
            if 'from apricot.constants import *' not in content:
                new_content = imports + '\n' + content
                with open(path, 'w', encoding='utf-8') as f:
                    f.write(new_content)
                print(f"Fixed imports in {path}")
