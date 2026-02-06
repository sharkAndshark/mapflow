# MapFlow 行为测试清单（可观测契约）

本文档仅记录系统对外可观测行为，用于约束快速演进中的行为一致性。

## 行为清单（当前已覆盖）

| 行为 | 触发方式 | 期望结果 |
| --- | --- | --- |
| 上传 GeoJSON 后列表出现新文件 | UI 上传 `.geojson` | 列表出现该文件，状态为“已上传” |
| 上传 Shapefile zip 后列表出现新文件 | UI 上传 `.zip` | 列表出现该文件，状态为“已上传” |
| 首次进入首页显示历史上传文件 | 首次打开首页 | 列表显示历史上传记录 |
| 上传后重启服务仍可见历史文件 | 上传完成后重启服务 | 列表仍显示之前上传的文件 |
| 上传不支持格式时提示失败 | 上传不支持的文件 | 显示失败提示 |
| 上传超过配置上限时提示失败 | 上传超限文件 | 显示失败提示 |
| Shapefile zip 缺关键文件时上传失败 | 上传缺关键文件的 zip | 显示失败提示 |
| 选中文件点击预览打开新标签页 | 侧栏中的“Open Preview”按钮 | 打开新标签页，URL 形如 `/preview/:id`，且页面不返回 404 |
| 预览页加载地图瓦片 | 在预览页查看网络请求 | 包含 `/api/files/:id/tiles/z/x/y` 请求，状态码 200，响应头 `Content-Type: application/vnd.mapbox-vector-tile`，且至少有一个 Tile 响应体非空 |
| 点击列表行显示侧栏详情 | 点击文件列表任一行 | 右侧出现详情栏，展示 name/type/size/status/uploadedAt；若有错误展示 error |
| 文件状态自动刷新（轮询） | 上传后不刷新页面等待 | 状态从“等待处理/处理中”自动变为“已就绪” |

## 行为清单（计划补充）
暂无

## 一致性约束
- 行为测试通过即代表用户层面的承诺仍成立
- README 中的“最小接口/存储约定”变更时，应同步更新本文档

## 代码质量约束（CI 门禁）
- Rust：必须通过 `cargo fmt -- --check` 与 `cargo clippy -- -D warnings`
- Frontend：必须通过 Biome 格式检查（`biome format .` 或 `biome ci .`）

## 测试注意事项
- E2E 测试为每个 Playwright worker 启动独立后端进程，使用独立的 `PORT/DB_PATH/UPLOAD_DIR`，避免并发时 DB 锁与文件串扰
- 测试专用接口：`POST /api/test/reset` 仅在 debug 构建且设置 `MAPFLOW_TEST_MODE=1` 时注册；release 构建永不包含该接口

## CI 安全门禁
- CI 会构建并启动 release 二进制，然后请求 `POST /api/test/reset`，期望返回非 2xx（通常为 404/405）
