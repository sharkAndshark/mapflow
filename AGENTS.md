t1. 当你学到重要的经验教学，可以更新`AGENTS.md`，这是你的长期记忆。
2. 整个系统基于readme.md这个文档进行演进迭代，冲刺，只有当readme.md过分膨胀时，我们才会考虑将其拆散整合成mdbook文件夹。
3. 整个系统基于可观测行为进行测试，保证项目质量。当你完成一次功能/bug fix等时，记得review并更新readme.md 以及  TESTS.md 两个文件。
4. 如果遇到网络问题，考虑使用中国镜像，或者看看7897端口有没有代理可以用。
5. 使用cargo fmt and cargo clippy 来消除代码异味
6. 对duckdb spatial 的使用，务必参考 /Users/zhangyijun/RiderProjects/duckdb-spatial/docs 文件下的内容，尤其是空间函数如st_asmvt st_transform 等。
7. 永远不需要考虑用户更新版本和历史数据迁移的问题，当前软件还未发布，这些问题不存在