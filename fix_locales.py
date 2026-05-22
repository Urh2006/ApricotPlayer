import re
with open('locales.py', 'r', encoding='utf-8') as f:
    content = f.read()

# Fix the missing comma issue before search_provider
content = re.sub(r'"\s*\n\s*"search_provider"', '",\n        "search_provider"', content)

with open('locales.py', 'w', encoding='utf-8') as f:
    f.write(content)
print('Fixed missing commas.')
