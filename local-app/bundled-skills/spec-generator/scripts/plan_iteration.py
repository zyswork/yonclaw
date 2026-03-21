#!/usr/bin/env python3
"""迭代规划器：基于角色资源约束 + 优先级 + 依赖关系，推荐最优迭代范围。

用法:
  plan_iteration.py <requirements.json> <resources.json>

resources.json 格式:
  {"backend": {"count": 1, "days": 22}, "frontend": {"count": 1, "days": 22}, "test": {"count": 1, "days": 22}}

  或简单模式:
  {"team_size": 3, "days": 22}
"""
import json, sys, os
from collections import defaultdict

PRIORITY_WEIGHT = {"P0": 10, "P1": 6, "P2": 3}

def topo_sort(requirements, dep_graph):
    """拓扑排序，确保依赖项排在前面"""
    id_map = {r["id"]: r for r in requirements if r.get("id")}
    in_degree = defaultdict(int)
    adj = defaultdict(list)
    all_ids = set(id_map.keys())

    for rid, deps in dep_graph.items():
        for d in deps:
            if d in all_ids:
                adj[d].append(rid)
                in_degree[rid] += 1

    queue = [rid for rid in all_ids if in_degree[rid] == 0]
    queue.sort(key=lambda x: PRIORITY_WEIGHT.get(id_map.get(x, {}).get("priority", ""), 0), reverse=True)
    ordered = []
    while queue:
        node = queue.pop(0)
        ordered.append(node)
        for neighbor in adj[node]:
            in_degree[neighbor] -= 1
            if in_degree[neighbor] == 0:
                queue.append(neighbor)
                queue.sort(key=lambda x: PRIORITY_WEIGHT.get(id_map.get(x, {}).get("priority", ""), 0), reverse=True)

    return ordered

def parse_effort(r):
    """解析需求的各角色工时"""
    def to_float(v):
        try: return float(v) if v else 0
        except (ValueError, TypeError): return 0

    be = to_float(r.get("effort_backend", 0))
    fe = to_float(r.get("effort_frontend", 0))
    qa = to_float(r.get("effort_test", 0))
    total = to_float(r.get("effort", 0))

    # 如果有分角色工时，用分角色的
    if be + fe + qa > 0:
        return {"backend": be, "frontend": fe, "test": qa, "total": be + fe + qa}
    # 否则用总工时
    return {"backend": 0, "frontend": 0, "test": 0, "total": total}

def parse_resources(res):
    """解析资源配置，支持简单模式和角色模式"""
    if "backend" in res or "frontend" in res or "test" in res:
        roles = {}
        for role in ["backend", "frontend", "test"]:
            if role in res:
                r = res[role]
                roles[role] = r["count"] * r["days"] if isinstance(r, dict) else float(r)
            else:
                roles[role] = 0
        roles["total"] = sum(roles.values())
        return roles
    else:
        total = int(res.get("team_size", 3)) * int(res.get("days", 22))
        return {"backend": total / 3, "frontend": total / 3, "test": total / 3, "total": total}

def plan_iteration(requirements, dep_graph, resources):
    """生成迭代规划建议，按角色分别计算容量"""
    capacity = parse_resources(resources)
    id_map = {r["id"]: r for r in requirements if r.get("id")}
    sorted_ids = topo_sort(requirements, dep_graph)

    # 评分
    scored = []
    for rid in sorted_ids:
        r = id_map.get(rid)
        if not r:
            continue
        priority = r.get("priority", "P2")
        effort = parse_effort(r)
        weight = PRIORITY_WEIGHT.get(priority, 3)
        dep_penalty = 0.8 if rid in dep_graph else 1.0
        score = (weight * dep_penalty) / max(effort["total"], 0.5)
        scored.append({
            "id": rid,
            "name": r.get("name", ""),
            "module": r.get("module", ""),
            "priority": priority,
            "effort": effort,
            "score": round(score, 2),
            "dependencies": dep_graph.get(rid, []),
        })

    # 排序：P0优先，再按score
    def sort_key(item):
        p_order = {"P0": 0, "P1": 1, "P2": 2}
        return (p_order.get(item["priority"], 3), -item["score"])
    scored.sort(key=sort_key)

    # 贪心选择：按角色分别检查容量
    selected = []
    selected_ids = set()
    remaining_items = []
    used = {"backend": 0, "frontend": 0, "test": 0, "total": 0}

    def fits(effort):
        """检查是否在各角色容量内"""
        has_role_data = effort["backend"] + effort["frontend"] + effort["test"] > 0
        if has_role_data:
            for role in ["backend", "frontend", "test"]:
                if capacity[role] > 0 and used[role] + effort[role] > capacity[role]:
                    return False
        if used["total"] + effort["total"] > capacity["total"]:
            return False
        return True

    def add(item):
        e = item["effort"]
        for role in ["backend", "frontend", "test", "total"]:
            used[role] += e.get(role, 0)
        selected.append(item)
        selected_ids.add(item["id"])

    for item in scored:
        deps_met = all(d in selected_ids for d in item["dependencies"])
        if deps_met and fits(item["effort"]):
            add(item)
        else:
            remaining_items.append(item)

    # 二次扫描
    changed = True
    while changed:
        changed = False
        still_remaining = []
        for item in remaining_items:
            deps_met = all(d in selected_ids for d in item["dependencies"])
            if deps_met and fits(item["effort"]):
                add(item)
                changed = True
            else:
                still_remaining.append(item)
        remaining_items = still_remaining

    # 利用率
    utilization = {}
    for role in ["backend", "frontend", "test"]:
        if capacity[role] > 0:
            utilization[role] = {
                "used": used[role],
                "capacity": capacity[role],
                "percent": round(used[role] / capacity[role] * 100, 1),
                "is_bottleneck": used[role] / capacity[role] > 0.9,
            }
    utilization["total"] = {
        "used": used["total"],
        "capacity": capacity["total"],
        "percent": round(used["total"] / capacity["total"] * 100, 1) if capacity["total"] > 0 else 0,
    }

    # 风险分析
    risks = []
    for role, data in utilization.items():
        if role != "total" and isinstance(data, dict) and data.get("is_bottleneck"):
            risks.append(f"⚠️ {role} 利用率 {data['percent']}%，是瓶颈角色")

    for item in selected:
        if item["effort"]["total"] >= capacity.get("total", 999) * 0.3:
            risks.append(f"⚠️ {item['id']}({item['name']}) 工时 {item['effort']['total']}天，占迭代 >30%，建议拆分")
        if item["dependencies"]:
            dep_names = ", ".join(item["dependencies"])
            risks.append(f"🔗 {item['id']} 依赖 {dep_names}，需确保先完成")

    # 分阶段（按优先级+模块自动分组）
    phases = []
    phase1 = [i for i in selected if i["module"] in ["技术优化", "基础设施", "infrastructure", "tech"]]
    phase2 = [i for i in selected if i not in phase1 and i["priority"] == "P0"]
    phase3 = [i for i in selected if i not in phase1 and i not in phase2]

    if phase1:
        phases.append({"name": "Phase 1: 技术基础", "items": phase1, "effort": sum(i["effort"]["total"] for i in phase1)})
    if phase2:
        phases.append({"name": "Phase 2: 核心功能", "items": phase2, "effort": sum(i["effort"]["total"] for i in phase2)})
    if phase3:
        phases.append({"name": "Phase 3: 增强功能", "items": phase3, "effort": sum(i["effort"]["total"] for i in phase3)})

    # 如果没有技术优化模块，就按P0/P1/P2分
    if not phases:
        p0 = [i for i in selected if i["priority"] == "P0"]
        p1 = [i for i in selected if i["priority"] == "P1"]
        p2 = [i for i in selected if i["priority"] == "P2"]
        if p0: phases.append({"name": "Phase 1: P0 需求", "items": p0, "effort": sum(i["effort"]["total"] for i in p0)})
        if p1: phases.append({"name": "Phase 2: P1 需求", "items": p1, "effort": sum(i["effort"]["total"] for i in p1)})
        if p2: phases.append({"name": "Phase 3: P2 需求", "items": p2, "effort": sum(i["effort"]["total"] for i in p2)})

    return {
        "capacity": capacity,
        "selected": [{"id": i["id"], "name": i["name"], "module": i["module"], "priority": i["priority"],
                       "effort": i["effort"], "dependencies": i["dependencies"]} for i in selected],
        "selected_count": len(selected),
        "utilization": utilization,
        "remaining": [{"id": i["id"], "name": i["name"], "priority": i["priority"],
                        "effort": i["effort"]} for i in remaining_items],
        "remaining_count": len(remaining_items),
        "risks": risks,
        "phases": phases,
    }

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: plan_iteration.py <requirements.json> <resources.json>", file=sys.stderr)
        print("  resources.json: {\"backend\":{\"count\":1,\"days\":22},\"frontend\":{\"count\":1,\"days\":22},\"test\":{\"count\":1,\"days\":22}}", file=sys.stderr)
        print("  or simple:      {\"team_size\":3,\"days\":22}", file=sys.stderr)
        sys.exit(1)

    with open(sys.argv[1]) as f:
        data = json.load(f)

    # 第二个参数可以是 JSON 文件或 JSON 字符串
    res_arg = sys.argv[2]
    if os.path.exists(res_arg):
        with open(res_arg) as f:
            resources = json.load(f)
    else:
        resources = json.loads(res_arg)

    result = plan_iteration(
        data["requirements"],
        data.get("dependency_graph", {}),
        resources
    )
    print(json.dumps(result, ensure_ascii=False, indent=2))
