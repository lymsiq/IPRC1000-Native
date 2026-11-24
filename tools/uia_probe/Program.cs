// UIA 探测工具（2026-11-20）——只读诊断，不注入、不改系统。
// 目的：回答三个问题
//   1. WorkBuddy（Electron/WebView2）的 UIA 树有多深、能否拿到内部控件？
//   2. 输入框（ControlType.Edit）有没有稳定可定位的属性（AutomationId/Name/ClassName）？
//   3. ★ 用户正常点输入框时，当前焦点控件的属性能否被读到？——决定"无感学习"可行性。
//
// 用法：
//   uia_probe.exe              # 探测 WorkBuddy 主窗口的控件树（找 Edit）
//   uia_probe.exe --focus      # 读当前焦点控件 + 祖先链（无感学习的关键验证）
//   uia_probe.exe --pid 17172  # 指定进程
//   uia_probe.exe --all        # 列出所有顶层窗口

using System.Text;
using System.Windows.Automation;

Console.OutputEncoding = Encoding.UTF8;

static string Safe(Func<string?> f) { try { return f() ?? ""; } catch { return ""; } }
static string Rect(System.Windows.Rect r) => $"[{r.X:F0},{r.Y:F0} {r.Width:F0}x{r.Height:F0}]";

var argv = Environment.GetCommandLineArgs().Skip(1).ToList();

if (argv.Contains("--all"))
{
    var root = AutomationElement.RootElement;
    var wins = root.FindAll(TreeScope.Children, Condition.TrueCondition);
    Console.WriteLine($"顶层窗口 {wins.Count} 个：");
    foreach (AutomationElement w in wins)
    {
        var name = Safe(() => w.Current.Name);
        var cls = Safe(() => w.Current.ClassName);
        var pid = Safe(() => w.Current.ProcessId.ToString());
        if (name.Contains("WorkBuddy", StringComparison.OrdinalIgnoreCase)
            || cls.Contains("Chrome", StringComparison.OrdinalIgnoreCase))
            Console.WriteLine($"  pid={pid,-6} class={cls,-28} name={name}");
    }
    return;
}

if (argv.Contains("--focus"))
{
    AutomationElement? el = null;
    try { el = AutomationElement.FocusedElement; } catch { }
    if (el is null) { Console.WriteLine("拿不到当前焦点控件。"); return; }

    Console.WriteLine("=== 当前焦点控件（祖先链，从焦点往根）===");
    var cur = el;
    var depth = 0;
    var walk = TreeWalker.RawViewWalker;
    while (cur is not null && depth < 12)
    {
        var name = Safe(() => cur.Current.Name);
        var id = Safe(() => cur.Current.AutomationId);
        var cls = Safe(() => cur.Current.ClassName);
        var type = Safe(() => cur.Current.ControlType.ProgrammaticName);
        var focusable = Safe(() => cur.Current.IsKeyboardFocusable.ToString());
        var rect = Safe(() => Rect(cur.Current.BoundingRectangle));
        Console.WriteLine($"  [{depth}] {type,-22} focusable={focusable,-5} id=\"{id}\" cls=\"{cls}\" name=\"{name}\" {rect}");
        AutomationElement? parent = null;
        try { parent = walk.GetParent(cur); } catch { }
        cur = parent;
        depth++;
    }
    return;
}

// 默认：探测 WorkBuddy 主窗口
int pidTarget = 0;
var pi = argv.IndexOf("--pid");
if (pi >= 0 && pi + 1 < argv.Count) int.TryParse(argv[pi + 1], out pidTarget);
if (pidTarget == 0)
{
    var procs = System.Diagnostics.Process.GetProcessesByName("WorkBuddy")
        .Where(p => !string.IsNullOrEmpty(p.MainWindowTitle)).ToList();
    if (procs.Count == 0) { Console.WriteLine("找不到 WorkBuddy 主窗口进程（先打开 WorkBuddy）。"); return; }
    pidTarget = procs[0].Id;
    Console.WriteLine($"用 WorkBuddy 主窗口进程 pid={pidTarget}  title={procs[0].MainWindowTitle}");
}

AutomationElement? win = null;
try
{
    var root = AutomationElement.RootElement;
    var cond = new PropertyCondition(AutomationElement.ProcessIdProperty, pidTarget);
    var found = root.FindAll(TreeScope.Children, cond);
    if (found.Count > 0) win = found[0];
}
catch (Exception e) { Console.WriteLine($"找窗口失败: {e.Message}"); return; }
if (win is null) { Console.WriteLine("没找到该进程的顶层窗口。"); return; }

Console.WriteLine($"窗口: name=\"{Safe(() => win.Current.Name)}\" cls=\"{Safe(() => win.Current.ClassName)}\" {Safe(() => Rect(win.Current.BoundingRectangle))}");

// 收集：树深度/总节点/Edit 控件
var walker = TreeWalker.RawViewWalker;
int total = 0, maxDepth = 0, editCount = 0;
var edits = new List<AutomationElement>();
var candidates = new List<(int depth, string line)>();
var typeCount = new Dictionary<string, int>();
var focusableSamples = new List<string>();

void Walk(AutomationElement el, int depth)
{
    if (depth > 25 || total > 3000) return;
    total++;
    if (depth > maxDepth) maxDepth = depth;
    try
    {
        var type = el.Current.ControlType.ProgrammaticName;
        var name = Safe(() => el.Current.Name);
        var id = Safe(() => el.Current.AutomationId);
        var cls = Safe(() => el.Current.ClassName);
        var focusable = el.Current.IsKeyboardFocusable;

        var tkey = type.Replace("ControlType.", "");
        typeCount[tkey] = typeCount.GetValueOrDefault(tkey) + 1;
        if (focusable && focusableSamples.Count < 15)
            focusableSamples.Add($"{tkey} id=\"{id}\" name=\"{name}\"");

        // 记录所有"可能可聚焦、有名字"的控件（输入候选）
        // ★ 不要求 IsKeyboardFocusable：Electron/WebView2 里子树控件的 focusable 全是 false，
        //   只有根 Pane 是真。定位器靠 ControlType/id/name，不靠 focusable。
        if (type.Contains("Edit") || type.Contains("Document")
            || name.Contains("输入", StringComparison.OrdinalIgnoreCase)
            || name.Contains("input", StringComparison.OrdinalIgnoreCase)
            || name.Contains("搜索", StringComparison.OrdinalIgnoreCase)
            || name.Contains("发消息", StringComparison.OrdinalIgnoreCase)
            || name.Contains("问", StringComparison.OrdinalIgnoreCase))
        {
            var rect = Safe(() => Rect(el.Current.BoundingRectangle));
            var line = $"d={depth,-2} {type,-22} id=\"{id}\" cls=\"{cls}\" name=\"{name}\" {rect}";
            candidates.Add((depth, line));
            if (type.Contains("Edit")) { editCount++; edits.Add(el); }
        }

        var child = walker.GetFirstChild(el);
        while (child is not null)
        {
            Walk(child, depth + 1);
            child = walker.GetNextSibling(child);
        }
    }
    catch { /* 控件失效/跨进程，跳过 */ }
}

Walk(win, 0);
Console.WriteLine($"\n树统计: 节点 {total}  最大深度 {maxDepth}  Edit 控件 {editCount}  候选 {candidates.Count}");
Console.WriteLine("\n=== 控件类型分布 ===");
foreach (var kv in typeCount.OrderByDescending(k => k.Value).Take(15))
    Console.WriteLine($"  {kv.Key,-20} {kv.Value}");
Console.WriteLine($"\n=== 可聚焦控件样本（共采样 {focusableSamples.Count}）===");
foreach (var s in focusableSamples) Console.WriteLine("  " + s);

Console.WriteLine("\n=== 输入候选控件（可聚焦 + Edit/Document/名字含输入/搜索）===");
foreach (var (d, line) in candidates.Take(40)) Console.WriteLine("  " + line);
if (candidates.Count > 40) Console.WriteLine($"  ... 还有 {candidates.Count - 40} 个");

if (edits.Count > 0)
{
    Console.WriteLine("\n=== Edit 控件属性明细（前 10 个，判断属性是否稳定）===");
    foreach (var e in edits.Take(40))
    {
        Console.WriteLine($"  id=\"{Safe(() => e.Current.AutomationId)}\"  cls=\"{Safe(() => e.Current.ClassName)}\"  name=\"{Safe(() => e.Current.Name)}\"  framework=\"{Safe(() => e.Current.FrameworkId)}\"");
    }
}

Console.WriteLine("\n=== 结论判据 ===");
Console.WriteLine(editCount == 0
    ? "  ✗ 拿不到 Edit 控件 ⇒ Electron/WebView2 树太粗，走锚点坐标方案"
    : $"  ✓ 拿到 {editCount} 个 Edit ⇒ 可用 UIA locator；检查上面的 id/name 是否稳定（不含时间戳/随机串）");
