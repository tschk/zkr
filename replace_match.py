import re

with open('src/store/embeddings.rs', 'r') as f:
    content = f.read()

# Let's see if the file doesn't have a newline at the end and cargo fmt is failing on that?
# The error was:
# Diff in /home/runner/work/zkr/zkr/src/store/embeddings.rs:640:
#      }))
#  }
#
# -
#  fn extract_target_revision_and_text(
#      target_kind: &str,
#      s_rev: Option<i64>,

content = content.replace("}\n\nfn extract_target_revision_and_text", "}\nfn extract_target_revision_and_text")

with open('src/store/embeddings.rs', 'w') as f:
    f.write(content)
