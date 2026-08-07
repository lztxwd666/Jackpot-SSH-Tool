# 循环依赖检测（开发期工具）：扫描每个 crate 内模块级 use 关系找环
# 用法：python scripts/check_rust_deps.py
import os
import re
import sys

sys.stdout.reconfigure(encoding='utf-8', errors='replace')

CRATES = ['core-common', 'core-event', 'core-storage', 'core-runtime', 'desktop']


def main():
    found = False
    for crate in CRATES:
        root = f'crates/{crate}/src'
        if not os.path.isdir(root):
            continue
        mods = {}
        for dirpath, _, fnames in os.walk(root):
            for fn in fnames:
                if fn.endswith('.rs') and fn not in ('lib.rs', 'main.rs'):
                    p = os.path.join(dirpath, fn)
                    rel = os.path.relpath(p, root).replace('.rs', '')
                    rel = rel.replace(os.sep, '/')
                    mods[p] = rel
        # 收集 crate 内引用（use crate::X / use super::X 的顶层模块）
        edges = set()
        for f in mods:
            with open(f, encoding='utf-8', errors='replace') as fh:
                for line in fh:
                    m = re.match(r'\s*use\s+(crate|super)::([a-z_0-9]+)', line)
                    if m:
                        target = m.group(2)
                        for tf, rel in mods.items():
                            if rel.split('/')[0] == target:
                                src_top = mods[f].split('/')[0]
                                dst_top = rel.split('/')[0]
                                if src_top != dst_top:
                                    edges.add((src_top, dst_top))
        tops = sorted(set(e[0] for e in edges) | set(e[1] for e in edges))
        g = {t: set() for t in tops}
        for src, dst in edges:
            g[src].add(dst)

        # DFS 找环
        white, gray, black = 0, 1, 2
        color = {t: white for t in g}
        cycles = []
        stack = []

        def dfs(u):
            color[u] = gray
            stack.append(u)
            for v in sorted(g[u]):
                if color[v] == gray:
                    idx = stack.index(v)
                    cycles.append(' -> '.join(stack[idx:] + [v]))
                elif color[v] == white:
                    dfs(v)
            stack.pop()
            color[u] = black

        for t in g:
            if color[t] == white:
                dfs(t)

        print(f'== {crate}: 顶层模块引用 {sorted(g.items())}')
        if cycles:
            found = True
            print('  循环依赖:')
            for c in sorted(set(cycles)):
                print('   ', c)
        else:
            print('  循环依赖: 无')
    print()
    print('结论:', '发现循环依赖' if found else '全部 crate 模块级无循环依赖')
    sys.exit(1 if found else 0)


if __name__ == '__main__':
    main()
