## 原始输入（原文，勿改）
- 1.配置添加的时候增加配置简称说明和分类,分类信息也使用json做到配置中去维护

> 状态: ✅ 已完成 (6/6)
> 分支: `worktree-todo-001-match-meta`
> 更新: 2026-08-17
> 设计: `docs/plans/2026-08-17-match-short-name-description-category-design.md`

## 任务清单

- ✅ T1. 扩展 `UiMatch` / `StoredMatch`：`short_name`；`ui_id` 与 `label` 语义分离；旧文件兼容加载
- ✅ T2. 新增 `meta.rs`：`Category`、`UiMetaStore`，读写 `match/ui-meta.json`
- ✅ T3. `UiMatchRepository` 联合 load/save（yml + meta）；孤儿 meta 清理
- ✅ T4. `SettingsModel`：categories、category filter、搜索覆盖简称/说明
- ✅ T5. Slint：表单字段 + 列表简称/分类 + 分类筛选 + 新建分类
- ✅ T6. 测试更新 + clippy/fmt；勾验收

## 目标与范围

- **场景**：Settings → Configuration → 新增/编辑匹配时填写简称、说明、分类；分类 JSON 落在配置目录。
- **持久化**：
  - 简称 → `match/ui.yml` 的 `label`（espanso Search Bar 同步受益）
  - 稳定 id → `match/ui.yml` 的 `ui_id`
  - 说明 + 分类引用 + 分类目录 → `match/ui-meta.json`
- **明确不做**：多标签、分类删除/重命名管理页、外部 match 回写、引擎 schema 扩展。

## 验收要点

- [x] 新增配置表单出现：简称、说明、分类控件
- [x] 列表可用分类筛选；简称优先于 trigger 展示（有则）
- [x] 分类 Create 持久化到配置目录 JSON，重启后仍在
- [x] Export/Import/Open/Create 不破坏该 JSON（整树备份自动带走）
- [x] 无分类/无简称的旧条目仍可正常编辑

## 说明与上下文（完成后补）

- 做了什么：
  - `UiMatch` 增加 `short_name` / `description` / `category_id`。
  - `ui.yml` 用 `ui_id` 作稳定主键，`label` 仅表示简称（兼容旧文件把 `label=ui-…` 当 id）。
  - 新增 `match/ui-meta.json` 存分类目录与每条说明/分类引用。
  - Configuration 表单：Short name → Trigger → Replacement → Description → Category + Add category。
  - 列表主标题优先简称，支持分类下拉筛选；搜索覆盖简称/说明。
- 关键决策：
  - 简称走官方 `label`，Search Bar 直接受益；分类/说明走 sidecar JSON，不污染引擎 schema。
  - 单分类（组）而非 multi-tag，贴合用户「分类」措辞且实现更小。
  - 新建分类立即写 meta，不依赖当前草稿 Save。
  - 一期不做分类删除/重命名 UI。
- 涉及文件：
  - `espanso-settings/src/{meta.rs,matches.rs,model.rs,app.rs,lib.rs}`
  - `espanso-settings/ui/settings.slint`
  - `espanso-settings/tests/{matches.rs,model.rs,meta.rs}`
  - `docs/plans/2026-08-17-match-short-name-description-category-design.md`
  - `docs/todo_excluded/todo_001.md`
- 验证结果：
  - `cargo test -p espanso-settings --no-default-features` 全过（含 meta 3 / matches 6 / model 6）
  - `cargo clippy -p espanso-settings --no-default-features --all-targets -- -D warnings` 零警告
  - `cargo fmt -p espanso-settings` 已对齐
  - `cargo check -p espanso-settings --features ui` 通过
- 风险与后续：
  - 分类删除/重命名管理页未做。
  - 手改 YAML 丢掉 `ui_id` 会丢 meta 关联。
  - 三端真实窗口冒烟未做。
