#!/usr/bin/env python3
"""扫描任意项目目录，自动识别技术栈、模块结构、API、数据库表等上下文信息。"""
import json, sys, os, re, glob

def detect_tech_stack(root):
    """通过配置文件检测技术栈"""
    indicators = {
        # Java
        "pom.xml": {"lang": "Java", "build": "Maven", "cmd": "mvn clean compile"},
        "build.gradle": {"lang": "Java/Kotlin", "build": "Gradle", "cmd": "gradle build"},
        "build.gradle.kts": {"lang": "Kotlin", "build": "Gradle KTS", "cmd": "gradle build"},
        # JavaScript/TypeScript
        "package.json": {"lang": "JavaScript/TypeScript", "build": "npm/yarn", "cmd": "npm run build"},
        "tsconfig.json": {"lang": "TypeScript"},
        # Python
        "requirements.txt": {"lang": "Python", "build": "pip"},
        "pyproject.toml": {"lang": "Python", "build": "poetry/pip"},
        "setup.py": {"lang": "Python", "build": "setuptools"},
        # Go
        "go.mod": {"lang": "Go", "build": "go", "cmd": "go build ./..."},
        # Rust
        "Cargo.toml": {"lang": "Rust", "build": "cargo", "cmd": "cargo build"},
        # .NET
        "*.csproj": {"lang": "C#", "build": ".NET", "cmd": "dotnet build"},
        "*.sln": {"lang": "C#", "build": ".NET"},
        # Docker
        "Dockerfile": {"container": "Docker"},
        "docker-compose.yml": {"container": "Docker Compose"},
        "docker-compose.yaml": {"container": "Docker Compose"},
    }

    stack = {"languages": set(), "frameworks": set(), "build_tools": set(),
             "commands": {}, "databases": set(), "containers": set()}

    for filename, info in indicators.items():
        if "*" in filename:
            matches = glob.glob(os.path.join(root, filename))
            if not matches:
                matches = glob.glob(os.path.join(root, "**", filename), recursive=True)
            found = len(matches) > 0
        else:
            found = os.path.exists(os.path.join(root, filename))
            if not found:  # 检查子目录
                found = len(glob.glob(os.path.join(root, "**", filename), recursive=True)) > 0

        if found:
            if "lang" in info: stack["languages"].add(info["lang"])
            if "build" in info: stack["build_tools"].add(info["build"])
            if "cmd" in info: stack["commands"][info["build"]] = info["cmd"]
            if "container" in info: stack["containers"].add(info["container"])

    # 检测框架（从 pom.xml / package.json）
    pom = os.path.join(root, "pom.xml")
    if os.path.exists(pom):
        try:
            content = open(pom).read()
            if "spring-boot" in content: stack["frameworks"].add("Spring Boot")
            if "mybatis" in content: stack["frameworks"].add("MyBatis")
            if "hibernate" in content: stack["frameworks"].add("Hibernate")
        except: pass

    pkg = os.path.join(root, "package.json")
    if os.path.exists(pkg):
        try:
            p = json.load(open(pkg))
            deps = {**p.get("dependencies", {}), **p.get("devDependencies", {})}
            fw_map = {"react": "React", "vue": "Vue", "next": "Next.js", "nuxt": "Nuxt",
                      "angular": "Angular", "express": "Express", "nestjs": "NestJS",
                      "fastify": "Fastify", "electron": "Electron", "svelte": "Svelte"}
            for key, name in fw_map.items():
                if any(key in d for d in deps): stack["frameworks"].add(name)
            # 检测测试/构建命令
            scripts = p.get("scripts", {})
            if "build" in scripts: stack["commands"]["build"] = f"npm run build"
            if "test" in scripts: stack["commands"]["test"] = f"npm test"
            if "dev" in scripts: stack["commands"]["dev"] = f"npm run dev"
        except: pass

    # 数据库检测
    for f in glob.glob(os.path.join(root, "**", "*.sql"), recursive=True)[:1]:
        stack["databases"].add("SQL")
    for f in glob.glob(os.path.join(root, "**", "*.prisma"), recursive=True)[:1]:
        stack["databases"].add("Prisma")

    # 转为可序列化
    return {k: sorted(list(v)) if isinstance(v, set) else v for k, v in stack.items()}


def scan_directory_structure(root, max_depth=3):
    """扫描目录结构"""
    structure = []
    root = os.path.abspath(root)
    skip_dirs = {".git", "node_modules", "__pycache__", ".idea", ".vscode",
                 "target", "build", "dist", ".next", "vendor", ".gradle", "venv", ".env"}

    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in skip_dirs]
        depth = dirpath.replace(root, "").count(os.sep)
        if depth >= max_depth:
            dirnames.clear()
            continue
        rel = os.path.relpath(dirpath, root)
        if rel == ".": rel = ""
        structure.append({"path": rel or ".", "type": "dir", "files": len(filenames), "dirs": len(dirnames)})

    return structure


def find_docs(root):
    """查找项目中的文档文件"""
    doc_patterns = ["*.md", "*.txt", "*.rst", "*.adoc"]
    skip_dirs = {".git", "node_modules", "__pycache__", "target", "build", "dist", "vendor"}
    docs = []

    for pattern in doc_patterns:
        for f in glob.glob(os.path.join(root, "**", pattern), recursive=True):
            if any(skip in f for skip in skip_dirs):
                continue
            rel = os.path.relpath(f, root)
            size = os.path.getsize(f)
            if size > 100:  # 跳过空文件
                docs.append({"path": rel, "size": size, "size_human": f"{size/1024:.1f}KB"})

    docs.sort(key=lambda x: x["size"], reverse=True)
    return docs[:50]  # 最多返回50个


def find_api_hints(root):
    """从代码中提取 API 端点线索"""
    api_patterns = [
        (r'@(Get|Post|Put|Delete|Patch)Mapping\s*\(\s*["\']([^"\']+)', "Spring"),
        (r'@(RequestMapping)\s*\(\s*(?:value\s*=\s*)?["\']([^"\']+)', "Spring"),
        (r'(app|router)\.(get|post|put|delete|patch)\s*\(\s*["\']([^"\']+)', "Express"),
        (r'@(api_view|action)\s*\(\s*\[([^\]]+)', "Django"),
    ]
    apis = []
    code_exts = {".java", ".py", ".js", ".ts", ".kt", ".go", ".cs"}

    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in {".git", "node_modules", "target", "build", "dist"}]
        for fname in filenames:
            ext = os.path.splitext(fname)[1]
            if ext not in code_exts:
                continue
            fpath = os.path.join(dirpath, fname)
            try:
                content = open(fpath, errors="ignore").read()
                for pattern, framework in api_patterns:
                    for match in re.finditer(pattern, content):
                        groups = match.groups()
                        endpoint = groups[-1]
                        apis.append({"endpoint": endpoint, "file": os.path.relpath(fpath, root), "framework": framework})
            except: pass

    return apis[:100]  # 最多100个


def find_db_tables(root):
    """从 SQL 文件或 ORM 模型中提取表名"""
    tables = set()

    # SQL 文件
    for f in glob.glob(os.path.join(root, "**", "*.sql"), recursive=True):
        try:
            content = open(f, errors="ignore").read()
            for m in re.finditer(r'CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?[`"\']?(\w+)', content, re.I):
                tables.add(m.group(1))
        except: pass

    # Java Entity/DO
    for f in glob.glob(os.path.join(root, "**", "*.java"), recursive=True):
        try:
            content = open(f, errors="ignore").read()
            for m in re.finditer(r'@Table\s*\(\s*name\s*=\s*["\'](\w+)', content):
                tables.add(m.group(1))
        except: pass

    return sorted(list(tables))[:100]


def scan_project(root):
    """主扫描函数"""
    root = os.path.abspath(root)
    if not os.path.isdir(root):
        return {"error": f"Not a directory: {root}"}

    result = {
        "project_root": root,
        "project_name": os.path.basename(root),
        "tech_stack": detect_tech_stack(root),
        "directory_structure": scan_directory_structure(root),
        "documents": find_docs(root),
        "api_endpoints": find_api_hints(root),
        "db_tables": find_db_tables(root),
    }

    # 摘要
    result["summary"] = {
        "languages": result["tech_stack"]["languages"],
        "frameworks": result["tech_stack"]["frameworks"],
        "doc_count": len(result["documents"]),
        "api_count": len(result["api_endpoints"]),
        "table_count": len(result["db_tables"]),
        "dir_count": len(result["directory_structure"]),
    }

    return result


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: scan_project.py <project_directory>", file=sys.stderr)
        sys.exit(1)

    result = scan_project(sys.argv[1])
    print(json.dumps(result, ensure_ascii=False, indent=2))
