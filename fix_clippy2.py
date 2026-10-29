import re

with open('src/store/retrieval.rs', 'r') as f:
    content = f.read()

# Replace format!(...) with "..." where format! is no longer needed

# In lexical_targets:
content = content.replace('&format!("SELECT s.id', '"SELECT s.id')
# The ends are `LIMIT ?4"),` and `LIMIT ?6"),`.
# BUT wait! we need to match only what was changed. It's better to just do exact string replacements.

pattern1 = '&format!("SELECT s.id, c.id\n                 FROM source_fts'
replacement1 = '"SELECT s.id, c.id\n                 FROM source_fts'
content = content.replace(pattern1, replacement1)

# we also need to remove the `)` at the end of the format call.
content = content.replace('LIMIT ?4"),', 'LIMIT ?4",')
content = content.replace('LIMIT ?6"),', 'LIMIT ?6",')

# For the String ones in claim_lexical_targets:
# `format!(\n                    "SELECT id`
pattern2 = 'format!(\n                    "SELECT id\n                     FROM ('
replacement2 = '"SELECT id\n                     FROM ('.replace('"', '"').to_string() # wait, needs .to_string()
# actually, since the type is `String`, we can just replace `format!(...)` with `"...".to_string()`.

content = content.replace('format!(\n                    "SELECT id', '"SELECT id')
content = content.replace('LIMIT ?4"\n                ),', 'LIMIT ?4".to_string(),')
content = content.replace('LIMIT ?6"\n                ),', 'LIMIT ?6".to_string(),')

with open('src/store/retrieval.rs', 'w') as f:
    f.write(content)
