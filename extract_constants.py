import sys
with open('wx_main.py', 'r', encoding='utf-8') as f:
    lines = f.readlines()

# Find the end of constants (line 699 is usually the last constant before classes/functions)
end_idx = 0
for i, line in enumerate(lines):
    if line.startswith('def default_equalizer_gains'):
        end_idx = i
        break

constants = lines[:end_idx]
remaining = lines[end_idx:]

with open('apricot/constants.py', 'w', encoding='utf-8') as f:
    f.writelines(constants)

# We need to prepend imports to wx_main.py
new_wx_main = ["from apricot.constants import *\n"] + remaining
with open('wx_main.py', 'w', encoding='utf-8') as f:
    f.writelines(new_wx_main)

print(f"Extracted {end_idx} lines of constants.")
