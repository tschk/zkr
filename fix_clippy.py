import re

with open('src/store/retrieval.rs', 'r') as f:
    content = f.read()

# Replace format!(...) with "..." or &"..." where it occurs
content = content.replace('&format!("SELECT', '"SELECT')
content = content.replace('LIMIT ?4"),', 'LIMIT ?4",')
content = content.replace('LIMIT ?6"),', 'LIMIT ?6",')
content = content.replace('format!(\n                    "SELECT', '"SELECT')

with open('src/store/retrieval.rs', 'w') as f:
    f.write(content)
