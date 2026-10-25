import sys

def main():
    with open('src/store/projects.rs', 'r') as f:
        lines = f.readlines()

    for i, line in enumerate(lines):
        if "categories.truncate(limit);" in line and "fn dream_aggregate_categories" in "".join(lines[i-60:i]):
            del lines[i]
            break

    for i, line in enumerate(lines):
        if "let schemas = Self::dream_aggregate_schemas" in line:
            lines.insert(i, "        categories.truncate(limit);\n")
            break

    with open('src/store/projects.rs', 'w') as f:
        f.writelines(lines)

if __name__ == '__main__':
    main()
