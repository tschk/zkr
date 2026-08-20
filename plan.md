1. **Understand & Assess**: The vulnerability allows an attacker to specify an arbitrary command in `options.command`, which is then passed to `child_process.spawn`. This could lead to Arbitrary Code Execution if a malicious user can control `options` (e.g., via `openclaw.json` or other plugin configs). Since `shell: true` is not used, it's executable path injection rather than full shell injection, but it's still dangerous (e.g., pointing it to `node`, `python`, `curl` etc.).
2. **Implement Fix**:
   - Modify `plugins/openclaw/cli.ts` to validate `options.command`.
   - Ensure it's a string, does not contain null bytes (`\0`), and its `basename` is strictly `"zkr"` or `"zkr.exe"`.
   - If validation fails, return a rejected Promise with `ZKR_COMMAND_FAILED` to prevent leaking information.
3. **Update Tests**:
   - Update `plugins/openclaw/index.test.ts` so the tests that use `"echo"`, `"false"`, or `"zkr-command-that-must-not-exist"` use validly named temporary files (named `"zkr"`) to continue correctly exercising `spawn` error handling and stream output behavior, maintaining the integrity of the test suite.
4. **Pre-commit Steps**:
   - Run `pre_commit_instructions` to ensure proper testing, verification, review, and reflection are done.
5. **Submit**:
   - Create a PR with a security-focused PR title and description.
