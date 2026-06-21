# Agent Output Compact Fork Strategy

本仓库是基于 RTK 的本地 dogfood fork，用来验证一个更适合 Codex Desktop / agent shell 场景的输出压缩工具。

当前目标不是从零重写 RTK，也不是完整合并 Headroom；目标是保留 RTK 的命令适配能力，同时吸收 Headroom 中少数对可靠性有帮助的机制。

## 基本判断

RTK 的价值在于命令级 adapter：`git`、`grep/rg`、JSON、测试/构建输出等命令知道自己的语义，所以压缩质量比通用截断更可靠。

Headroom 的价值不在 Kompress 模型，而在几个工程思想：

- 原文缓存，压缩后仍可取回完整输出。
- 压缩 metadata，说明使用了哪个 adapter、压缩前后大小和节省比例。
- 无收益或解析失败时 passthrough，不强行改写。
- 对大型 JSON/数组结果做结构化保留，而不是只截断文本。

## 非目标

第一阶段不做这些：

- 不引入 Kompress / ModernBERT / 任何模型推理。
- 不做 LLM API proxy。
- 不做复杂 MCP server。
- 不把 Content Router 作为主架构。
- 不先扩展几十个新命令 adapter。
- 不依赖 Codex Desktop hook 作为主路径。

## 主架构

主路径仍然是 RTK 风格：

```text
command adapter -> command execution -> adapter-specific compaction
```

未知命令才进入轻量 fallback：

```text
unknown command output -> obvious shape classifier -> safe compaction or passthrough
```

形态识别只处理非常确定的输出：

- unified diff
- grep-like `file:line:content`
- valid JSON
- obvious test/log output
- generic long text

不确定时不要聪明处理，直接 passthrough 或头尾截断并保留原文。

## 第一批融合功能

### 1. Raw Cache

压缩前保存原始 stdout/stderr/exit code 到用户级缓存目录，例如：

```text
C:/Users/Administrator/.codex/compact-cache/
```

压缩输出末尾附带短标记：

```text
[compact: adapter=git.diff raw=184KB shown=12KB saved=93% raw=abc123]
```

后续提供取回命令：

```powershell
compact raw show abc123
```

### 2. Metadata

每次压缩记录：

- adapter 名称
- 原始 stdout/stderr 字节数
- 压缩后字节数
- 节省比例
- 耗时
- exit code
- 是否 passthrough
- raw cache id

metadata 既用于输出尾部，也用于本地统计。

### 3. Passthrough Guard

如果 adapter 解析失败、压缩后没有明显变小、命令风险太高或输出本身很短，则直接原样输出。

原则：

```text
错误压缩比不压缩更糟。
```

### 4. JSON Array Compaction

借鉴 Headroom 对结构化结果的处理思路：

- 保留顶层 schema。
- 大数组只展示前后样本。
- 优先保留包含 `error`、`failed`、`warning`、`exception` 的项。
- 输出明确说明省略了多少项。
- 原文缓存可取回完整 JSON。

## Dogfood 流程

先保留 RTK 已有 adapter，不急着重命名所有内部符号。实际使用中按下面方式迭代：

1. 用现有高收益命令。
2. 遇到压缩质量差、误判、Windows 不兼容或耗时异常时记录案例。
3. 同类问题重复出现，才新增或修改 adapter。
4. 修改后用同一条命令对比 raw output、RTK output、fork output。
5. 确认收益稳定后再更新用户规则。

记录问题时保留这几项：

```text
date:
command:
cwd:
raw size:
compact size:
adapter:
problem:
expected behavior:
raw cache id:
```

## 初始优先级

第一优先级：

- `git status`
- `git log`
- `git diff`
- `git show`
- `rg/grep`
- JSON 文件/JSON stdout

第二优先级：

- 测试失败输出
- 构建日志
- Windows 下路径和命令兼容性

暂缓：

- 多 agent hook 生态
- 完整 trust/integrity 扩展
- 远程 telemetry
- 大规模命令矩阵

## 命名与许可证

RTK 和 Headroom 都是 Apache 2.0。fork 可以改名和发布衍生版本，但必须保留原始许可证和 attribution。

本 fork 后续需要另起产品名，避免和官方 RTK 混淆。改名应分阶段做：

1. 先改 binary/package display name。
2. 再改 README 和安装文档。
3. 最后再考虑内部模块名。

不要在第一天为了改名大规模重写路径和符号。

## 当前工作目录

```text
D:/agent-output-compact
```

这是长期本地工作区，不使用临时目录。
