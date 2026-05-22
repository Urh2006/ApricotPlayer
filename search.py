from pathlib import Path

file_path = Path(r"C:\Users\urhst\.gemini\antigravity\scratch\ApricotPlayer\wx_main.py")

query = "def bind_player_navigation_control"
print(f"Searching for '{query}':")
found_idx = -1
with open(file_path, "r", encoding="utf-8", errors="ignore") as f:
    lines = f.readlines()

for idx, line in enumerate(lines, 1):
    if query in line:
        found_idx = idx
        break

if found_idx != -1:
    print(f"Found on line {found_idx}")
    # Print the next 40 lines
    for i in range(found_idx - 1, min(found_idx + 40, len(lines))):
        print(f"{i+1}: {lines[i].rstrip()}")
else:
    print("Not found.")
