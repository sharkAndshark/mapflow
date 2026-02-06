1. 当你学到重要的经验教学，可以更新`AGENTS.md`，这是你的长期记忆。
2. 整个系统基于 `docs/dev.md` 及其子文档（尤其是 `docs/dev/behaviors.md`）进行演进迭代。`README.md` 仅作为用户入口，不用于记录冲刺任务或内部细节。
3. 整个系统基于可观测行为进行测试，保证项目质量。当你完成一次功能/bug fix等时，记得 review 并更新 `docs/dev/testing.md`（原 TESTS.md）以及 `docs/dev/behaviors.md`。
4. 如果遇到网络问题，考虑使用中国镜像，或者看看7897端口有没有代理可以用。
5. 使用 cargo fmt and cargo clippy 来消除代码异味。
6. 对 duckdb spatial 的使用，务必参考 `/Users/zhangyijun/RiderProjects/duckdb-spatial/docs` 文件下的内容，尤其是空间函数如 `st_asmvt` `st_transform` 等。
7. 永远不需要考虑用户更新版本和历史数据迁移的问题，当前软件还未发布，这些问题不存在。
8. 做正确的决策，不要折衷，不要最小改动。当前软件还未发布，我们不要欠技术债。
9. commit前运行测试
10. 你对包括AGENTS.md 以及 docs 的更新，是项目快速、稳健、正确演进的发动机，正向自循环的那种

附：当你改变了可观测契约（API 状态码/错误文案/流程语义）或测试策略时，必须同步更新：
- `docs/dev/behaviors.md`（契约）
- `docs/dev/testing.md`（验证方式/命令）
