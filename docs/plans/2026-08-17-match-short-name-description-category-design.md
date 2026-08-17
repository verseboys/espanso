# 配置简称 / 说明 / 分类（JSON 维护）设计

> 日期: 2026-08-17  
> 风险: medium（扩展 Settings 可写匹配模型与 UI；不改匹配引擎语义）  
> 关联: `docs/todo_excluded/todo_001.md`、`docs/plans/2026-08-12-adopt-existing-config-and-echo-matches-design.md`、`docs/plans/2026-08-15-config-portable-backup-and-folder-intents-design.md`

## 1. 目标

用户诉求（原文）：

> 配置添加的时候增加配置简称说明和分类,分类信息也使用json做到配置中去维护

| # | 交付 | 说明 |
|---|------|------|
| 1 | 简称 + 说明 | 新增/编辑表单除 trigger / replace 外可填短名与备注 |
| 2 | 分类选择/新建 | 可选分类；可即时新建分类 |
| 3 | 分类 JSON 落盘 | 分类目录与条目元数据写在配置目录内 JSON，随备份/迁移带走 |

**非目标：**

- 不改 espanso 核心匹配引擎 / 搜索栏算法
- 不把外部只读 match 变成可写
- 不做多标签（multi-tag）、云同步、分类颜色图标一期
- 不强制手写 YAML 必须带简称/分类

## 2. 调研结论（deep）

SearXNG token 本机不可用，本轮以官方文档 + 产品文档 + WebSearch 交叉验证。

| # | 结论 | 可信度 | 对方案的约束 | 来源 |
|---|------|--------|--------------|------|
| 1 | Snippet 产品普遍区分 **人类可读名** 与 **触发串**：TextExpander Label vs Abbreviation；Raycast Name vs Keyword | ⭐⭐⭐ | 列表/表单主展示用简称，trigger 仍是扩展键 | [TextExpander best practices](https://textexpander.com/blog/textexpander-best-practices)、[Raycast Snippets](https://manual.raycast.com/snippets) |
| 2 | 组织模型两派：**文件夹/组**（Alfred Collections、TextExpander Groups）vs **标签**（Raycast Tags） | ⭐⭐⭐ | 一期单分类（组）足够；比 multi-tag 更贴「分类」措辞，实现更小 | [Alfred Collections](https://www.alfredapp.com/help/features/snippets/collections/)、[Raycast Snippets](https://manual.raycast.com/snippets) |
| 3 | **espanso 官方已有 `label`**：Search Bar 描述；**没有** category 字段 | ⭐⭐⭐ | 简称写入 `label`，Search Bar 同步受益；分类不要硬塞进引擎 schema | [Matches basics](https://espanso.org/docs/matches/basics/) |
| 4 | 社区 GUI（espanso_gui 等）写 label 回 YAML 时易丢复杂字段；Settings 已承诺外部只读 | ⭐⭐⭐ | 只写我们托管的 `match/ui.yml` + 自有 JSON | [espanso_gui](https://github.com/Pebkac03/espanso_gui)、本仓库 2026-08-12 设计 |
| 5 | **Sidecar 元数据**（TagSpaces / SnipVault）把 tags/categories 放 JSON 与内容并列，便于移植与版本控制 | ⭐⭐⭐ | 分类目录 + 说明/分类引用放 JSON；与「分类用 JSON 维护」原话一致 | [TagSpaces meta formats](https://docs.tagspaces.org/dev/metafileformats/) |
| 6 | 表单字段顺序：名称 → 触发 → 内容 → 组织属性（分类/备注）是常见编辑器布局 | ⭐⭐ | 表单顺序：简称 → 触发 → 替换 → 说明 → 分类 | Raycast / TextExpander 编辑流 |

### 2.1 现状硬约束（代码）

- `UiMatch { id, trigger, replace }`；`id` 现经 `ui.yml` 的 `label` 往返（`matches.rs`）。
- 列表行 `MatchRow` 只显示 `trigger` + `preview`。
- 表单只有 Trigger / Replacement。
- 保存只写 `match/ui.yml`；Export 整树打包，**新 JSON 会自动进入备份**（无需改 BackupService 逻辑，只要文件在配置根下）。
- `espanso-config` 的 `YAMLMatch` **无** `deny_unknown_fields`，未知键被忽略，可安全写入 `ui_id` 供 Settings 稳定身份。

## 3. 方案决策

### 3.1 字段语义

| UI 字段 | 英文 id | 必填 | 持久化 | 用途 |
|---------|---------|------|--------|------|
| 简称 | `short_name` | 否 | `match/ui.yml` → `label` | 列表主标题、Search Bar 描述；空则列表回退到 trigger |
| 说明 | `description` | 否 | `match/ui-meta.json` → matches[id].description | 备注，不参与匹配 |
| 分类 | `category_id` | 否 | `match/ui-meta.json` → matches[id].category_id | 引用 categories[].id；空=未分类 |
| （内部）id | `id` | 是 | `match/ui.yml` → `ui_id` | 稳定主键；**不再**占用 `label` |
| Trigger / Replace | 同现网 | 是 / 是 | `ui.yml` | 匹配语义不变 |

### 3.2 JSON 文件

路径：`{config_root}/match/ui-meta.json`

```json
{
  "version": 1,
  "categories": [
    { "id": "cat-…", "name": "工作", "order": 0 }
  ],
  "matches": {
    "ui-…": {
      "description": "客服工单回复落款",
      "category_id": "cat-…"
    }
  }
}
```

规则：

- 文件不存在 ≡ 空 categories + 空 matches 元数据（兼容旧配置）。
- 保存时：只保留仍存在于 `ui.yml` 的 match id；孤儿元数据清理。
- 分类删除：条目 `category_id` 清空为未分类，不级联删 match（一期不做删除 UI）。
- 分类名去首尾空白；同名（大小写不敏感）拒绝新建。
- `id` 用 `cat-{millis}` / 既有 `ui-{millis}`，避免用户改名导致引用断裂。

### 3.3 `ui.yml` 形状（Settings 托管）

```yaml
# Managed by Espanso Settings. Advanced rules may be edited in other files.
matches:
  - ui_id: ui-1723…
    label: 工单签名          # 可选；有简称才写
    trigger: ":sig"
    replace: |
      …
```

兼容加载：

| 磁盘条目 | id | short_name |
|----------|----|------------|
| 有 `ui_id`，有 `label` | ui_id | label |
| 有 `ui_id`，无 `label` | ui_id | "" |
| 无 `ui_id`，`label` 形如 `ui-…` 或 `ui-match-…` | label | "" |
| 无 `ui_id`，其它 `label` | 生成稳定回退 `ui-match-{index}` 并在下次保存写回 ui_id | label |
| 两者皆无 | `ui-match-{index}` | "" |

### 3.4 UI / UX

Configuration 页：

1. **列表**
   - 主行：`short_name` 非空则显示简称，旁注 trigger；否则显示 trigger。
   - 次行：replace 预览；有分类时附分类名。
   - 顶部：文本搜索（含简称/说明/trigger/replace）+ 分类下拉（All / Uncategorized / 各分类）。

2. **表单顺序**  
   Short name → Trigger → Replacement → Description → Category  
   - Category：`ComboBox` = Uncategorized + 已有分类。  
   - 旁路：`New category` 输入框 + `Add`：写入内存分类列表，随下次 Save match 或独立 add 立即写 meta。  
   - **一期只实现 Add + 下拉选择**，不做分类删除/重命名管理页。

3. **外部只读行**  
   - 不绑 meta 编辑；不写 JSON。

### 3.5 模型与服务

```
UiMatch { id, trigger, replace, short_name, description, category_id }
Category { id, name, order }
UiMetaDocument { version, categories, matches: Map<id, MatchMeta> }
UiMetaStore { load/save at match/ui-meta.json }
```

`UiMatchRepository::load/save` 联合读写 `ui.yml` + meta：

1. 校验并原子写 `ui.yml`（现逻辑）
2. 再原子写 `ui-meta.json`
3. meta 失败 surface error，提示重试（yml 已是新数据，重试只补 meta）

`SettingsModel`：

- `categories: Vec<Category>`
- `category_filter: CategoryFilter { All | Uncategorized | Id(String) }`
- `filtered_matches` 叠加 category_filter，并搜索 short_name/description
- `add_category(name) -> Result<Category>`

### 3.6 备份 / 脚手架 / 接纳

| 路径 | 行为 |
|------|------|
| Export / Import / Migrate copy | 整树，自动含 `ui-meta.json` |
| Scaffold 新建 | 不强制写空 meta（懒创建） |
| Adopt | 零写入；若已有 meta 则加载 |

## 4. 验收

- [ ] 新增表单可见 Short name / Description / Category，可保存重开回显
- [ ] 列表显示简称（有则优先），分类筛选可用
- [ ] `match/ui-meta.json` 存在且 categories 可新增
- [ ] 旧 `ui.yml`（仅 label=id）不丢数据、不崩溃
- [ ] `cargo test -p espanso-settings --no-default-features` 全过
- [ ] clippy（headless）零警告

## 5. 实现顺序

1. 扩展 `UiMatch` + `StoredMatch`（`ui_id` / `label` 语义分离）+ 兼容加载
2. 新增 `meta.rs`：`Category` / `UiMetaStore`
3. `UiMatchRepository` 联合 load/save
4. `SettingsModel` 过滤与分类
5. Slint 表单/列表/筛选绑定
6. 测试 + 更新 todo 状态

## 6. 风险

- 旧文件 `label` 既可能是内部 id 也可能是用户手写简称：用 §3.3 启发式；下次保存写回 `ui_id`。
- `ui_id` 对引擎透明；手改 YAML 删掉 `ui_id` 会丢 meta 关联——可接受。
- 一期不做分类删除/重命名 UI。
