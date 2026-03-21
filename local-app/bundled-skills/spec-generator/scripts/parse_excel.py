#!/usr/bin/env python3
"""解析 Excel 需求表为结构化 JSON，自动检测列名映射。"""
import json, sys, os

try:
    import openpyxl
except ImportError:
    print("ERROR: openpyxl not installed. Run: pip3 install openpyxl", file=sys.stderr)
    sys.exit(1)

# 列名映射（支持中英文多种写法）
COLUMN_MAP = {
    "id": ["id", "编号", "需求id", "需求编号", "req_id"],
    "module": ["模块", "module", "功能模块", "所属模块"],
    "name": ["功能名称", "name", "需求名称", "feature", "标题", "title"],
    "description": ["需求描述", "description", "desc", "详细描述", "说明"],
    "priority": ["优先级", "priority", "级别", "重要程度"],
    "type": ["类型", "type", "需求类型", "分类"],
    "acceptance": ["验收标准", "acceptance", "acceptance criteria", "ac", "验收条件"],
    "dependency": ["依赖项", "dependency", "dependencies", "前置条件", "依赖"],
    "effort": ["预估工时", "effort", "工时", "人天", "预估工时(人天)", "estimate", "合计"],
    "effort_backend": ["后端工时", "backend", "后端", "后端(人天)", "be"],
    "effort_frontend": ["前端工时", "frontend", "前端", "前端(人天)", "fe"],
    "effort_test": ["测试工时", "test", "测试", "测试(人天)", "qa"],
    "note": ["备注", "note", "notes", "remark", "remarks"],
}

def detect_columns(header_row):
    """自动检测列名到标准字段的映射"""
    mapping = {}
    for col_idx, cell_val in enumerate(header_row):
        if not cell_val:
            continue
        val = str(cell_val).strip().lower()
        for std_name, aliases in COLUMN_MAP.items():
            if val in aliases and std_name not in mapping:
                mapping[std_name] = col_idx
                break
    return mapping

def parse_excel(filepath):
    wb = openpyxl.load_workbook(filepath, read_only=True, data_only=True)
    ws = wb.active

    rows = list(ws.iter_rows(values_only=True))
    if not rows:
        return {"error": "Empty spreadsheet", "requirements": []}

    # 检测表头（尝试前3行）
    mapping = {}
    header_row_idx = 0
    for i in range(min(3, len(rows))):
        mapping = detect_columns(rows[i])
        if len(mapping) >= 3:  # 至少匹配3个字段
            header_row_idx = i
            break

    if len(mapping) < 2:
        return {"error": f"Cannot detect columns. Header: {rows[0]}", "requirements": []}

    requirements = []
    for row in rows[header_row_idx + 1:]:
        if not any(row):  # 跳过空行
            continue
        req = {}
        for std_name, col_idx in mapping.items():
            val = row[col_idx] if col_idx < len(row) else None
            req[std_name] = str(val).strip() if val is not None else ""
        # 跳过无 ID 和无名称的行
        if not req.get("id") and not req.get("name"):
            continue
        requirements.append(req)

    wb.close()

    # 统计
    stats = {
        "total": len(requirements),
        "by_priority": {},
        "by_module": {},
        "by_type": {},
        "total_effort": 0,
    }
    for r in requirements:
        p = r.get("priority", "未设置")
        m = r.get("module", "未分类")
        t = r.get("type", "未分类")
        stats["by_priority"][p] = stats["by_priority"].get(p, 0) + 1
        stats["by_module"][m] = stats["by_module"].get(m, 0) + 1
        stats["by_type"][t] = stats["by_type"].get(t, 0) + 1
        try:
            stats["total_effort"] += float(r.get("effort", 0))
        except (ValueError, TypeError):
            pass

    # 依赖关系分析
    dep_graph = {}
    for r in requirements:
        rid = r.get("id", "")
        dep = r.get("dependency", "")
        if rid and dep:
            dep_graph[rid] = [d.strip() for d in dep.replace("，", ",").split(",") if d.strip()]

    return {
        "file": os.path.basename(filepath),
        "detected_columns": {k: list(COLUMN_MAP.keys())[list(COLUMN_MAP.values()).index(v)] if isinstance(v, list) else k for k, v in mapping.items()},
        "column_mapping": {k: v for k, v in mapping.items()},
        "stats": stats,
        "dependency_graph": dep_graph,
        "requirements": requirements,
    }

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: parse_excel.py <excel_file>", file=sys.stderr)
        sys.exit(1)

    filepath = sys.argv[1]
    if not os.path.exists(filepath):
        print(f"ERROR: File not found: {filepath}", file=sys.stderr)
        sys.exit(1)

    result = parse_excel(filepath)
    print(json.dumps(result, ensure_ascii=False, indent=2))
