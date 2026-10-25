import sys

def main():
    with open('src/store/projects.rs', 'r') as f:
        lines = f.readlines()

    for i, line in enumerate(lines):
        if "let categories = Self::dream_aggregate_categories" in line:
            lines[i] = line.replace("let categories", "let mut categories")
            break

    # also check if the reviewer's unused limit is a clippy warning!

    with open('src/store/projects.rs', 'w') as f:
        f.writelines(lines)

if __name__ == '__main__':
    main()
