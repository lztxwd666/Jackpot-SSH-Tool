#!/usr/bin/env python3
"""从 vscode-material-icon-theme（MIT）精选文件/文件夹图标入库并生成映射清单。

素材与映射的单一来源：本脚本内的精选列表既是复制清单，也导出为 manifest.ts，
前端 fileIcon.ts 直接消费 manifest，两者之间不存在第二份手工维护的映射。

用法: python scripts/fetch_material_icons.py [上游仓库路径]
路径缺省时浅克隆到临时目录（已存在则复用）。重复运行幂等，覆盖同名文件。

素材来源: https://github.com/material-extensions/vscode-material-icon-theme
许可: MIT（完整许可文本随素材一起入库，见 icons/material/LICENSE）
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

# 目标目录（仓库根下的前端资源）；素材按主题分类：assets/icons/themes/{theme}/{file,folder}
# 新增主题时在此新增同构目录（THEME_NAME 命名），并在前端注册对应 FileIconTheme
REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
THEME_NAME = "material"
TARGET = os.path.join(
    REPO_ROOT, "crates", "desktop", "ui", "src", "assets", "icons", "themes", THEME_NAME
)
UPSTREAM_REPO = "https://github.com/material-extensions/vscode-material-icon-theme"

# 上游默认蓝灰（folderColor/fileColor 的默认值 blue-gray-300）
DEFAULT_COLOR = "#90a4ae"
# 上游 generateOpenFolderIcons.ts 中的规范 open 文件夹路径（id="folder" 路径的 d 替换值）
OPEN_FOLDER_PATH = (
    "M14.483 6H4.721a1 1 0 0 0-.949.684L2 12V5h12a1 1 0 0 0-1-1H7.562a1 1 0 0 1-.64-.232"
    "l-.644-.536A1 1 0 0 0 5.638 3H2a1 1 0 0 0-1 1v8a1 1 0 0 0 1 1h11l2.403-5.606A1 1 0 0 0 14.483 6"
)
# 默认文件夹闭合路径（上游 folderGenerator.ts 的 folderIcon）
FOLDER_CLOSED_PATH = "m6.922 3.768-.644-.536A1 1 0 0 0 5.638 3H2a1 1 0 0 0-1 1v8a1 1 0 0 0 1 1h12a1 1 0 0 0 1 1V5a1 1 0 0 0-1-1H7.562a1 1 0 0 1-.64-.232"
# 默认文件图标路径（上游 fileGenerator.ts 的 fileIcon）
FILE_ICON_PATH = (
    "m8.668 6h3.6641l-3.6641-3.668v3.668m-4.668-4.668h5.332l4 4v8c0 0.73828-0.59375 1.3359-1.332 1.3359h-8c-0.73828 0"
    "-1.332-0.59766-1.332-1.3359v-10.664c0-0.74219 0.59375-1.3359 1.332-1.3359m3.332 1.3359h-3.332v10.664h8v-6h-4.668z"
)

# 扩展名到图标（图标名为上游 icons/ 目录下的文件名去后缀）
FILE_BY_EXT = {
    # 前端语言与框架
    "ts": "typescript",
    "mts": "typescript",
    "cts": "typescript",
    "tsx": "react_ts",
    "js": "javascript",
    "mjs": "javascript",
    "cjs": "javascript",
    "jsx": "react",
    "vue": "vue",
    "html": "html",
    "htm": "html",
    "css": "css",
    "scss": "sass",
    "sass": "sass",
    "less": "less",
    "json": "json",
    "jsonc": "json",
    "json5": "json",
    "yaml": "yaml",
    "yml": "yaml",
    "toml": "toml",
    "xml": "xml",
    "xsl": "xml",
    "xsd": "xml",
    "md": "markdown",
    "markdown": "markdown",
    "mdx": "mdx",
    "txt": "document",
    # 后端与系统语言
    "py": "python",
    "pyi": "python",
    "pyw": "python",
    "rs": "rust",
    "ron": "rust",
    "go": "go",
    "java": "java",
    "class": "javaclass",
    "jar": "jar",
    "c": "c",
    "h": "h",
    "cpp": "cpp",
    "cc": "cpp",
    "cxx": "cpp",
    "hpp": "hpp",
    "hh": "hpp",
    "hxx": "hpp",
    "cs": "csharp",
    "php": "php",
    "rb": "ruby",
    "gemspec": "ruby",
    "swift": "swift",
    "kt": "kotlin",
    "kts": "kotlin",
    "dart": "dart",
    "lua": "lua",
    "pl": "perl",
    "pm": "perl",
    "perl": "perl",
    "r": "r",
    "scala": "scala",
    "sol": "solidity",
    "hs": "haskell",
    "erl": "erlang",
    "ex": "elixir",
    # 脚本与配置
    "sh": "console",
    "bash": "console",
    "zsh": "console",
    "ksh": "console",
    "csh": "console",
    "tcsh": "console",
    "fish": "console",
    "ps1": "powershell",
    "psm1": "powershell",
    "psd1": "powershell",
    "bat": "exe",
    "cmd": "exe",
    "sql": "database",
    "db": "database",
    "sqlite": "database",
    "sqlite3": "database",
    "hcl": "hcl",
    "tf": "hcl",
    "tfvars": "hcl",
    "ini": "settings",
    "conf": "settings",
    "cfg": "settings",
    "properties": "settings",
    "env": "tune",
    "log": "log",
    # 文档与表格
    "csv": "table",
    "tsv": "table",
    "xls": "table",
    "xlsx": "table",
    "xlsm": "table",
    "ods": "table",
    "doc": "word",
    "docx": "word",
    "rtf": "word",
    "odt": "word",
    "ppt": "powerpoint",
    "pptx": "powerpoint",
    "pdf": "pdf",
    "epub": "epub",
    # 媒体
    "png": "image",
    "jpg": "image",
    "jpeg": "image",
    "gif": "image",
    "webp": "image",
    "bmp": "image",
    "ico": "image",
    "tiff": "image",
    "svg": "svg",
    "mp4": "video",
    "avi": "video",
    "mkv": "video",
    "mov": "video",
    "webm": "video",
    "flv": "video",
    "wmv": "video",
    "mp3": "audio",
    "wav": "audio",
    "flac": "audio",
    "ogg": "audio",
    "m4a": "audio",
    "aac": "audio",
    "wma": "audio",
    "ttf": "font",
    "otf": "font",
    "woff": "font",
    "woff2": "font",
    "eot": "font",
    # 压缩包与可执行文件
    "zip": "zip",
    "tar": "zip",
    "gz": "zip",
    "tgz": "zip",
    "rar": "zip",
    "7z": "zip",
    "bz2": "zip",
    "xz": "zip",
    "zst": "zip",
    "exe": "exe",
    "msi": "exe",
    "dll": "dll",
    "so": "dll",
    "ilk": "dll",
    "pem": "certificate",
    "crt": "certificate",
    "cer": "certificate",
    "key": "key",
    # 第二批补齐（日常高频补充；映射名以上游 icons/ 实际文件为准）
    "gradle": "gradle",
    "styl": "stylus",
    "lock": "lock",
    "psd": "adobe-photoshop",
    "ai": "adobe-illustrator",
    "sketch": "sketch",
    "fig": "figma",
    "blend": "blender",
    "obj": "3d",
    "fbx": "3d",
    "gltf": "3d",
    "glb": "3d",
    "stl": "3d",
    "3ds": "3d",
    "v": "vlang",
    "zig": "zig",
    "nim": "nim",
    "jl": "julia",
    "m": "objective-c",
    "mm": "objective-cpp",
    "groovy": "groovy",
    "asm": "assembly",
    "s": "assembly",
    "fs": "fsharp",
    "fsx": "fsharp",
    "clj": "clojure",
    "cljs": "clojure",
    "cljc": "clojure",
    "exs": "elixir",
    "hrl": "erlang",
    "mobi": "epub",
    "azw3": "epub",
    "avif": "image",
    "mpeg": "video",
    "mpg": "video",
    "webmanifest": "json",
    "p12": "certificate",
    "pfx": "certificate",
    "p7b": "certificate",
    "csr": "certificate",
    "apk": "android",
    "sln": "visualstudio",
}

# 精确文件名到图标（键小写；上游 fileNames 优先于扩展名匹配）
FILE_BY_NAME = {
    # 文档类（VSCode 惯例：README/LICENSE 等按文件名识别）
    "readme": "readme",
    "readme.md": "readme",
    "readme.txt": "readme",
    "readme.rst": "readme",
    "changelog": "changelog",
    "license": "license",
    "copying": "license",
    "authors": "authors",
    "contributing": "contributing",
    "codeowners": "codeowners",
    "todo": "todo",
    "credits": "credits",
    "conduct": "conduct",
    "security": "lock",
    "makefile": "makefile",
    "gnumakefile": "makefile",
    "cmakelists.txt": "cmake",
    "cmakecache.txt": "cmake",
    "cmakepresets.json": "cmake",
    "dockerfile": "docker",
    "containerfile": "docker",
    "gemfile": "gemfile",
    # 构建与依赖清单（覆盖同名配置的扩展名命中）
    "package.json": "nodejs",
    "package-lock.json": "nodejs",
    ".nvmrc": "nodejs",
    "yarn.lock": "yarn",
    "pnpm-lock.yaml": "pnpm",
    "pnpm-workspace.yaml": "pnpm",
    "bun.lockb": "bun",
    "bun.lock": "bun",
    "bunfig.toml": "bun",
    "deno.json": "deno",
    "deno.jsonc": "deno",
    "deno.lock": "deno",
    "poetry.lock": "poetry",
    "uv.lock": "uv",
    "uv.toml": "uv",
    "build.gradle": "gradle",
    "settings.gradle": "gradle",
    "pom.xml": "maven",
    "hosts": "hosts",
    "robots.txt": "robots",
    "favicon.ico": "favicon",
    "gitignore": "git",
    "gitattributes": "git",
    "gitmodules": "git",
    "gitlab-ci.yml": "gitlab",
    "biome.json": "biome",
    # 编辑器与工具配置
    "editorconfig": "editorconfig",
    "npmignore": "npm",
    "npmrc": "npm",
    "tsconfig.json": "tsconfig",
    "jsconfig.json": "jsconfig",
    "nginx.conf": "nginx",
    "prettierrc": "prettier",
    "eslintrc": "eslint",
    "babelrc": "babel",
    "tauri.conf.json": "tauri",
}

# 前缀匹配（配置类文件名常带后缀变体：.eslintrc.json、vite.config.ts 等）
FILE_BY_PREFIX = {
    ".eslintrc": "eslint",
    ".prettierrc": "prettier",
    ".babelrc": "babel",
    ".env": "tune",
    ".d.ts": "typescript-def",
    ".d.mts": "typescript-def",
    ".d.cts": "typescript-def",
    "vite.config": "vite",
    "vitest.config": "vitest",
    "webpack.config": "webpack",
    "tailwind.config": "tailwindcss",
    "jest.config": "jest",
    "playwright.config": "playwright",
    "cypress.config": "cypress",
    "svelte.config": "svelte",
    "astro.config": "astro",
    "next.config": "next",
    "nuxt.config": "nuxt",
    "tsconfig": "tsconfig",
    "dockerfile": "docker",
    "docker-compose": "docker",
}

# 文件夹名到图标（键小写；命中后取 folder-{name}[-open] 变体）
FOLDER_BY_NAME = {
    "src": "folder-src",
    "source": "folder-src",
    "sources": "folder-src",
    "code": "folder-src",
    "dist": "folder-dist",
    "build": "folder-dist",
    "out": "folder-dist",
    "output": "folder-dist",
    "outputs": "folder-dist",
    "release": "folder-dist",
    "bin": "folder-dist",
    "distribution": "folder-dist",
    "built": "folder-dist",
    "compiled": "folder-dist",
    "node_modules": "folder-node",
    "node": "folder-node",
    "public": "folder-public",
    "static": "folder-public",
    "assets": "folder-resource",
    "res": "folder-resource",
    "resource": "folder-resource",
    "report": "folder-resource",
    "reports": "folder-resource",
    "components": "folder-components",
    "test": "folder-test",
    "tests": "folder-test",
    "__tests__": "folder-test",
    "spec": "folder-test",
    "specs": "folder-test",
    "config": "folder-config",
    "configs": "folder-config",
    "git": "folder-git",
    ".github": "folder-github",
    "docs": "folder-docs",
    "doc": "folder-docs",
    "documentation": "folder-docs",
    "typescript": "folder-typescript",
    "ts": "folder-typescript",
    "rust": "folder-rust",
    "cargo": "folder-rust",
    "api": "folder-api",
    "functions": "folder-functions",
    "lambda": "folder-functions",
    "lib": "folder-lib",
    "library": "folder-lib",
    "utils": "folder-utils",
    "util": "folder-utils",
    "helpers": "folder-utils",
    "scripts": "folder-scripts",
    "css": "folder-css",
    "styles": "folder-css",
    "style": "folder-css",
    "stylesheets": "folder-css",
    "images": "folder-images",
    "image": "folder-images",
    "img": "folder-images",
    "video": "folder-video",
    "videos": "folder-video",
    "audio": "folder-audio",
    "sound": "folder-audio",
    "sounds": "folder-audio",
    "fonts": "folder-font",
    "font": "folder-font",
    "i18n": "folder-i18n",
    "lang": "folder-i18n",
    "locale": "folder-i18n",
    "locales": "folder-i18n",
    "language": "folder-i18n",
    "languages": "folder-i18n",
    "views": "folder-views",
    "packages": "folder-packages",
    "vue": "folder-vue",
    "database": "folder-database",
    "db": "folder-database",
    "log": "folder-log",
    "logs": "folder-log",
    "download": "folder-download",
    "downloads": "folder-download",
    "upload": "folder-upload",
    "uploads": "folder-upload",
    "archive": "folder-archive",
    "archives": "folder-archive",
    "tools": "folder-tools",
    "temp": "folder-temp",
    "tmp": "folder-temp",
    "keys": "folder-keys",
    "key": "folder-keys",
    "backup": "folder-backup",
    "backups": "folder-backup",
    "private": "folder-private",
    "src-tauri": "folder-src-tauri",
    "environment": "folder-environment",
    "env": "folder-environment",
    "environments": "folder-environment",
    "mock": "folder-mock",
    "mocks": "folder-mock",
    "coverage": "folder-coverage",
    "cov": "folder-coverage",
    "server": "folder-server",
    "client": "folder-client",
    "python": "folder-python",
    "py": "folder-python",
    "go": "folder-go",
    "golang": "folder-go",
    "java": "folder-java",
    "javascript": "folder-javascript",
    "js": "folder-javascript",
    "markdown": "folder-markdown",
    "md": "folder-markdown",
    "json": "folder-json",
    "docker": "folder-docker",
    "kubernetes": "folder-kubernetes",
    "k8s": "folder-kubernetes",
    "terraform": "folder-terraform",
    "aws": "folder-aws",
    "vercel": "folder-vercel",
    "graphql": "folder-graphql",
    "prisma": "folder-prisma",
    "hooks": "folder-hook",
    "hook": "folder-hook",
    "ci": "folder-ci",
    "workflows": "folder-gh-workflows",
    "project": "folder-project",
    "projects": "folder-project",
    "tasks": "folder-tasks",
    "task": "folder-tasks",
    "generators": "folder-generator",
    "generator": "folder-generator",
    "examples": "folder-examples",
    "example": "folder-examples",
    "template": "folder-template",
    "templates": "folder-template",
    "misc": "folder-other",
    "other": "folder-other",
}


def upstream_dir(path: str | None) -> str:
    """返回上游仓库目录：给定路径直接复用，否则浅克隆到临时目录。"""
    if path:
        icons_dir = os.path.join(path, "icons")
        if not os.path.isdir(icons_dir):
            sys.exit(f"无效的上游路径（缺 icons/ 目录）: {path}")
        return path
    cache = os.path.join(tempfile.gettempdir(), "material-icon-theme")
    if os.path.isdir(os.path.join(cache, "icons")):
        return cache
    print(f"浅克隆 {UPSTREAM_REPO} ...")
    subprocess.run(["git", "clone", "--depth", "1", UPSTREAM_REPO, cache], check=True)
    return cache


def git_head(path: str) -> str:
    """记录上游 commit（供来源追溯；非 git 目录时返回 unknown）。"""
    try:
        return subprocess.check_output(
            ["git", "-C", path, "rev-parse", "--short", "HEAD"], text=True
        ).strip()
    except Exception:
        return "unknown"


def svg_wrap(body: str) -> str:
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">{body}</svg>'


def open_variant(content: str) -> str:
    """上游 generateOpenFolderIcons.ts 的替换逻辑：将 id="folder" 路径的 d 换成规范 open 路径。"""
    return re.sub(
        r'(<path\s+id="folder"[^>]*\bd=")[^"]*(")',
        rf"\g<1>{OPEN_FOLDER_PATH}\g<2>",
        content,
    )


def collect_needed() -> tuple[set[str], set[str]]:
    """聚合所有需要的素材文件名（文件图标 + 文件夹图标及其 open 变体）。"""
    file_names = (
        {"file"}
        | set(FILE_BY_EXT.values())
        | set(FILE_BY_NAME.values())
        | set(FILE_BY_PREFIX.values())
    )
    folder_names = {"folder"} | set(FOLDER_BY_NAME.values())
    return file_names, folder_names


def copy_assets(upstream: str) -> None:
    file_needed, folder_needed = collect_needed()
    icons_dir = os.path.join(upstream, "icons")
    file_dir = os.path.join(TARGET, "file")
    folder_dir = os.path.join(TARGET, "folder")
    os.makedirs(file_dir, exist_ok=True)
    os.makedirs(folder_dir, exist_ok=True)

    missing = []
    for name in sorted(file_needed):
        src = os.path.join(icons_dir, f"{name}.svg")
        if name == "file":
            # 默认文件图标为上游运行时按主题颜色生成，仓库无源文件，此处按默认色生成
            content = svg_wrap(f'<path fill="{DEFAULT_COLOR}" d="{FILE_ICON_PATH}"/>')
            with open(os.path.join(file_dir, "file.svg"), "w", encoding="utf-8") as f:
                f.write(content)
            continue
        if not os.path.isfile(src):
            missing.append(f"{name}.svg")
            continue
        shutil.copyfile(src, os.path.join(file_dir, f"{name}.svg"))

    for name in sorted(folder_needed):
        src = os.path.join(icons_dir, f"{name}.svg")
        if name == "folder":
            # 默认文件夹为上游运行时按主题颜色生成，仓库无源文件，此处按默认色生成
            content = svg_wrap(
                f'<path id="folder" fill="{DEFAULT_COLOR}" d="{FOLDER_CLOSED_PATH}"/>'
            )
            open_content = svg_wrap(
                f'<path id="folder" fill="{DEFAULT_COLOR}" d="{OPEN_FOLDER_PATH}"/>'
            )
            with open(
                os.path.join(folder_dir, "folder.svg"), "w", encoding="utf-8"
            ) as f:
                f.write(content)
            with open(
                os.path.join(folder_dir, "folder-open.svg"), "w", encoding="utf-8"
            ) as f:
                f.write(open_content)
            continue
        if not os.path.isfile(src):
            missing.append(f"{name}.svg")
            continue
        shutil.copyfile(src, os.path.join(folder_dir, f"{name}.svg"))
        with open(
            os.path.join(folder_dir, f"{name}-open.svg"), "w", encoding="utf-8"
        ) as f:
            f.write(open_variant(open(src, encoding="utf-8").read()))

    if missing:
        sys.exit("上游缺少精选清单中的图标: " + ", ".join(sorted(missing)))

    # MIT 协议要求随副本附带版权声明与许可文本
    shutil.copyfile(os.path.join(upstream, "LICENSE"), os.path.join(TARGET, "LICENSE"))
    print(
        f"素材入库完成: {TARGET}（{len(file_needed)} 文件图标 + {len(folder_needed)} 文件夹图标含 open 变体）"
    )


def write_manifest(upstream: str) -> None:
    """生成 manifest.ts：前端 fileIcon.ts 的唯一映射来源，勿手改。"""
    header = (
        "// 由 scripts/fetch_material_icons.py 自动生成，请勿手改。\n"
        f"// 素材来源: vscode-material-icon-theme（MIT），上游 commit: {git_head(upstream)}\n"
        "// 仓库: https://github.com/material-extensions/vscode-material-icon-theme\n"
    )
    payload = {
        "byExtension": FILE_BY_EXT,
        "byName": FILE_BY_NAME,
        "byPrefix": FILE_BY_PREFIX,
        "folderByName": FOLDER_BY_NAME,
    }
    # 键已按书写顺序排列，json.dumps 保持插入序即可
    body = json.dumps(payload, ensure_ascii=False, indent=2)
    # 不带 as const：键值均为 string，消费方以 Record<string, string | undefined> 索引
    with open(os.path.join(TARGET, "manifest.ts"), "w", encoding="utf-8") as f:
        f.write(header + "export default " + body + "\n")
    print("manifest.ts 已生成")


def main() -> None:
    upstream = upstream_dir(sys.argv[1] if len(sys.argv) > 1 else None)
    copy_assets(upstream)
    write_manifest(upstream)


if __name__ == "__main__":
    main()
