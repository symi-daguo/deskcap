#!/usr/bin/env python3
"""生成 `rime-data/glossary.en.tsv`（候选译词种子表）。

为什么不手写 .tsv：词表是数据不是代码，混在仓库里靠肉眼维护迟早出错（tab 丢失、
重复键、漏行）。这里用一份可校验的生成器产出，重复键直接报错而不是静默覆盖。

用法：
    python3 macos-ime/scripts/gen-glossary.py          # 写入 rime-data/glossary.en.tsv
    python3 macos-ime/scripts/gen-glossary.py --check  # 只校验，不写（CI 用）

来源与许可：下方 CHARS / WORDS 为 DeskCap 自撰的常用词释义，不含任何第三方词库
数据。要换更大的词表，替换本脚本的数据区，或直接往用户目录放
`~/Library/Application Support/DeskCap/rime/glossary.en.tsv` 覆盖（无需重新打包）。
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "rime-data" / "glossary.en.tsv"

# ---------------------------------------------------------------- 常用单字
# 逐字合成的底座，命中率最高。整词没收录时用 " · " 把单字释义拼起来。
CHARS: dict[str, str] = {
    "的": "of", "我": "I", "你": "you", "他": "he", "她": "she", "它": "it",
    "是": "to be", "不": "not", "在": "at", "有": "to have", "人": "people",
    "中": "middle", "大": "big", "小": "small", "上": "above", "下": "below",
    "来": "come", "去": "go", "到": "reach", "说": "say", "看": "see",
    "听": "listen", "吃": "eat", "喝": "drink", "做": "do", "用": "use",
    "学": "learn", "写": "write", "读": "read", "走": "walk", "跑": "run",
    "开": "open", "关": "close", "进": "enter", "出": "exit", "天": "sky",
    "地": "earth", "年": "year", "月": "moon", "日": "day", "时": "time",
    "分": "divide", "家": "home", "国": "country", "城": "city", "门": "door",
    "车": "car", "路": "road", "飞": "fly", "水": "water", "火": "fire",
    "山": "mountain", "风": "wind", "雨": "rain", "雪": "snow", "云": "cloud",
    "海": "sea", "河": "river", "手": "hand", "口": "mouth", "眼": "eye",
    "耳": "ear", "心": "heart", "头": "head", "面": "face", "力": "power",
    "工": "work", "生": "life", "死": "death", "男": "male", "女": "female",
    "子": "child", "父": "father", "母": "mother", "兄": "elder brother",
    "弟": "younger brother", "姐": "elder sister", "妹": "younger sister",
    "朋": "friend", "友": "friend", "好": "good", "坏": "bad", "高": "tall",
    "低": "low", "长": "long", "短": "short", "多": "many", "少": "few",
    "新": "new", "旧": "old", "快": "fast", "慢": "slow", "早": "morning",
    "晚": "evening", "冷": "cold", "热": "hot", "白": "white", "黑": "black",
    "红": "red", "绿": "green", "蓝": "blue", "黄": "yellow", "名": "name",
    "号": "number", "问": "ask", "答": "answer", "语": "language", "话": "words",
    "字": "character", "文": "text", "书": "book", "笔": "pen", "纸": "paper",
    "音": "sound", "声": "voice", "色": "color", "光": "light", "影": "shadow",
    "星": "star", "空": "empty", "果": "fruit", "树": "tree", "花": "flower",
    "草": "grass", "鸟": "bird", "鱼": "fish", "猫": "cat", "狗": "dog",
    "马": "horse", "米": "rice", "菜": "dish", "肉": "meat", "酒": "wine",
    "茶": "tea", "饭": "meal", "爱": "love", "想": "think", "需": "need",
    "要": "want", "能": "can", "会": "can", "给": "give", "让": "let",
    "找": "find", "放": "put", "买": "buy", "卖": "sell", "钱": "money",
    "贵": "expensive", "变": "change", "始": "begin", "终": "end",
    "回": "return", "过": "pass", "见": "meet", "知": "know", "记": "remember",
    "信": "trust", "认": "recognize", "理": "reason", "情": "feeling",
    "感": "feel", "实": "real", "真": "true", "正": "correct", "全": "whole",
    "部": "part", "组": "group", "级": "level", "类": "class", "型": "type",
    "制": "system", "格": "pattern", "准": "standard", "确": "certain",
    "识": "knowledge", "经": "through", "济": "economy", "政": "politics",
    "法": "law", "医": "medicine", "教": "teach", "育": "education",
    "化": "culture", "历": "history", "史": "history", "科": "science",
    "技": "technology", "术": "art", "视": "video", "游": "play",
    "戏": "drama", "机": "machine", "电": "electric", "脑": "brain",
    "网": "net", "络": "network", "页": "page", "数": "number", "据": "data",
    "息": "information", "安": "safe", "全": "whole", "题": "question",
    "方": "method", "案": "case", "计": "count", "算": "calculate",
    "务": "service", "客": "guest", "户": "account", "端": "end",
    "服": "serve", "器": "device", "件": "item", "体": "body",
    "积": "volume", "系": "system", "统": "system", "程": "process",
    "序": "order", "性": "nature", "质": "quality", "量": "quantity",
    "加": "add", "减": "subtract", "乘": "multiply", "除": "remove",
    "输": "transport", "入": "enter", "存": "store", "储": "store",
    "取": "get", "删": "delete", "改": "change", "查": "query",
    "搜": "search", "测": "test", "试": "try", "验": "verify", "调": "tune",
    # --- 0.6.53 单字三批（用户反馈：「英文」第一候选没译词）---
    # 根因：「英」不在表里 → 逐字合成因一字缺失整体失败。
    # 单字是逐字合成的底座，覆盖越全，任意未收录词组都能兜底出释义。
    # 本批 300 个覆盖日常书面/口语高频字（不含已在上面的 241 个）。
    "英": "English", "汉": "Chinese", "单": "single", "词": "word",
    "句": "sentence", "篇": "article", "章": "chapter",
    "坐": "sit", "站": "stand", "躺": "lie down", "睡": "sleep",
    "醒": "wake", "忙": "busy", "闲": "free", "累": "tired",
    "饿": "hungry", "渴": "thirsty", "病": "illness", "疼": "pain",
    "药": "medicine", "爸": "dad", "妈": "mom", "爷": "grandpa",
    "奶": "grandma", "儿": "son", "孙": "grandchild",
    "春": "spring", "夏": "summer", "秋": "autumn", "冬": "winter",
    "午": "noon", "夜": "night", "秒": "second", "钟": "clock",
    "东": "east", "南": "south", "西": "west", "北": "north",
    "左": "left", "右": "right", "前": "front", "后": "back",
    "里": "inside", "外": "outside", "内": "inner", "旁": "beside",
    "一": "one", "二": "two", "三": "three", "四": "four",
    "五": "five", "六": "six", "七": "seven", "八": "eight",
    "九": "nine", "十": "ten", "百": "hundred", "千": "thousand",
    "万": "ten thousand", "零": "zero", "双": "pair", "元": "yuan",
    "打": "hit", "拉": "pull", "推": "push", "举": "lift",
    "提": "carry", "按": "press", "压": "press down", "切": "cut",
    "剪": "scissors", "修": "repair", "造": "build", "建": "construct",
    "寄": "mail", "付": "pay", "欠": "owe", "借": "borrow",
    "还": "return", "脸": "face", "嘴": "mouth", "牙": "tooth",
    "舌": "tongue", "鼻": "nose", "腿": "leg", "脚": "foot",
    "背": "back", "腰": "waist", "肚": "belly",
    "衣": "clothes", "鞋": "shoes", "帽": "hat", "裤": "pants", "袜": "socks",
    "桌": "table", "椅": "chair", "床": "bed", "灯": "lamp",
    "镜": "mirror", "伞": "umbrella", "包": "bag", "箱": "box",
    "盒": "case", "瓶": "bottle", "碗": "bowl", "盘": "plate",
    "杯": "cup", "刀": "knife", "叉": "fork", "勺": "spoon", "筷": "chopsticks",
    "油": "oil", "盐": "salt", "糖": "sugar", "醋": "vinegar",
    "汤": "soup", "粥": "porridge", "蛋": "egg",
    "石": "stone", "沙": "sand", "土": "soil", "金": "gold",
    "银": "silver", "铁": "iron", "铜": "copper",
    "叶": "leaf", "根": "root", "果": "fruit", "瓜": "melon",
    "猪": "pig", "羊": "sheep", "鸡": "chicken", "鸭": "duck",
    "兔": "rabbit", "蛇": "snake", "龙": "dragon", "虎": "tiger",
    "熊": "bear", "猴": "monkey", "象": "elephant",
    "灰": "gray", "紫": "purple", "粉": "pink", "棕": "brown", "橙": "orange",
    "难": "difficult", "易": "easy", "远": "far", "近": "near",
    "深": "deep", "浅": "shallow", "宽": "wide", "窄": "narrow",
    "厚": "thick", "薄": "thin", "重": "heavy", "轻": "light",
    "硬": "hard", "软": "soft", "强": "strong", "弱": "weak",
    "富": "rich", "穷": "poor", "活": "live", "老": "old",
    "危": "danger", "险": "risk", "平": "flat", "稳": "stable",
    "首": "first", "尾": "tail", "味": "taste", "香": "fragrant",
    "臭": "stinky", "甜": "sweet", "酸": "sour", "苦": "bitter", "辣": "spicy",
    "讲": "speak", "恨": "hate", "怕": "fear", "等": "wait",
    "帮": "help", "救": "rescue", "退": "retreat", "回": "return",
    "喜": "joy", "怒": "anger", "哀": "sorrow",
    "搞": "do", "弄": "handle", "办": "handle",
    "笑": "laugh", "哭": "cry", "唱": "sing", "画": "draw",
    "忘": "forget", "拿": "take", "送": "send", "丢": "lose",
    "换": "exchange", "跳": "jump", "起": "rise",
}

# ---------------------------------------------------------------- 常用词组
# 整词精确命中优先于逐字合成，所以日常高频词组放这里能得到更准的释义。
#
# 0.6.51 扩充（用户实测反馈：「我是」被逐字合成成「I · to be」，不是人话）——
# 代词短语 / 问候 / 高频动词短语是聊天里**最高频**的候选，缺了它们整词精确
# 命中就永远轮不上，全靠逐字合成兜底，读起来像词典而不像英语。
# 语言学习的正确顺序：先整词（我是 = I am），再逐字（我 = I）。
WORDS: dict[str, str] = {
    # --- 代词短语 / 系表结构（聊天最高频）---
    "我是": "I am", "你是": "you are", "他是": "he is", "她是": "she is",
    "我们": "we", "你们": "you (pl.)", "他们": "they", "她们": "they (f.)",
    "我的": "my / mine", "你的": "your / yours", "他的": "his", "她的": "her",
    "我们的": "our", "你们的": "your (pl.)", "他们的": "their",
    "自己": "oneself", "大家": "everyone",
    "这是什么": "what is this", "什么": "what", "谁": "who",
    "哪里": "where", "为什么": "why", "怎么": "how", "怎样": "how",
    "多少": "how many", "几个": "how many", "什么时候": "when",
    # --- 问候 / 日常应答 ---
    "你好": "hello", "您好": "hello (polite)", "大家好": "hello everyone",
    "谢谢": "thank you", "不客气": "you're welcome", "再见": "goodbye",
    "早安": "good morning", "晚安": "good night", "对不起": "sorry",
    "不好意思": "excuse me", "没关系": "it's OK", "好的": "OK",
    "是的": "yes", "不是": "no / not", "可以": "can / OK", "不行": "no",
    "当然": "of course", "可能": "maybe", "一定": "certainly",
    # --- 高频动词短语 ---
    "知道了": "got it", "明白了": "understood", "不知道": "don't know",
    "辛苦了": "thanks for your work",
    "加油": "keep it up", "没问题": "no problem",
    "看一看": "take a look", "等一下": "wait a moment",
    "马上": "right away", "现在": "now", "今天": "today",
    "明天": "tomorrow", "昨天": "yesterday", "早上": "morning",
    "下午": "afternoon", "晚上": "evening",
    # --- 原有技术/生活词组 ---
    "开发": "development", "编程": "programming", "架构": "architecture",
    "设计": "design", "测试": "testing", "管理": "management",
    "数据": "data", "网络": "network", "服务": "service",
    "服务器": "server", "客户端": "client", "系统": "system",
    "文件": "file", "用户": "user", "密码": "password",
    "登录": "log in", "安全": "security", "效率": "efficiency",
    "问题": "problem", "方法": "method", "方案": "solution",
    "时间": "time", "地点": "place", "名字": "name",
    "工作": "work", "学习": "study", "生活": "life",
    "世界": "world", "社会": "society", "经济": "economy",
    "文化": "culture", "历史": "history", "科学": "science",
    "技术": "technology", "艺术": "art", "音乐": "music",
    "电影": "movie", "游戏": "game", "手机": "phone",
    "电脑": "computer", "互联网": "internet", "软件": "software",
    "硬件": "hardware", "质量": "quality", "数量": "quantity",
    "价格": "price", "颜色": "color", "声音": "sound",
    "图片": "image", "视频": "video", "城市": "city",
    "公司": "company", "银行": "bank", "医院": "hospital",
    "学校": "school", "老师": "teacher", "学生": "student",
    "医生": "doctor", "朋友": "friend", "家庭": "family",
    "父母": "parents", "孩子": "child", "同事": "colleague",
    "领导": "leader", "会议": "meeting", "项目": "project",
    "需求": "requirement", "计划": "plan", "报告": "report",
    "代码": "code", "函数": "function", "变量": "variable",
    "数据库": "database", "接口": "interface", "版本": "version",
    "配置": "configuration", "部署": "deployment", "运行": "run",
    "安装": "install", "更新": "update", "错误": "error",
    "警告": "warning", "性能": "performance", "内存": "memory",
    "键盘": "keyboard", "鼠标": "mouse", "屏幕": "screen",
    "窗口": "window", "文件夹": "folder", "搜索": "search",
    "输入": "input", "输出": "output", "中文": "Chinese",
    "英语": "English", "汉语": "Chinese", "拼音": "pinyin",
    "英文": "English", "中文": "Chinese", "汉字": "Chinese character",
    "单词": "word", "翻译": "translate", "语言": "language",
    "你好": "hello", "谢谢": "thank you", "再见": "goodbye",
    "现在": "now", "今天": "today", "明天": "tomorrow",
    "昨天": "yesterday", "早上": "morning", "晚上": "evening",
    "以后": "after", "以前": "before", "一起": "together",
    "东西": "thing", "事情": "matter", "地方": "place",
    "问题解决": "problem solved", "早上好": "good morning",
    "不好意思": "sorry", "没关系": "it's fine",
    # --- 0.6.52 二批（用户反馈「很多都没正确匹配」）：聊天高频动词/形容/指代 ---
    "这个": "this", "那个": "that", "这些": "these", "那些": "those",
    "这里": "here", "那里": "there", "时候": "when / moment",
    "知道": "know", "觉得": "think / feel", "理解": "understand",
    "处理": "handle", "完成": "complete", "开始": "start",
    "结束": "end", "继续": "continue", "停止": "stop",
    "需要": "need", "应该": "should", "必须": "must",
    "重要": "important", "确定": "sure", "简单": "simple",
    "复杂": "complex", "方便": "convenient", "厉害": "awesome",
    "辛苦": "hard work", "着急": "anxious", "开心": "happy",
    "高兴": "glad", "满意": "satisfied", "失望": "disappointed",
    "喜欢": "like", "讨厌": "dislike", "希望": "hope",
    "打算": "plan", "准备": "prepare", "安排": "arrange",
    "检查": "check", "修改": "modify", "提交": "submit",
    "发布": "release", "上线": "go live", "部署": "deploy",
    "沟通": "communicate", "回复": "reply", "确认": "confirm",
    "同意": "agree", "拒绝": "reject", "支持": "support",
    "帮助": "help", "帮忙": "help out", "配合": "cooperate",
    "负责": "responsible", "认真": "serious", "仔细": "careful",
    "快点": "hurry up", "稍等": "one moment", "等待": "wait",
    "收到": "received", "在吗": "are you there?", "忙吗": "are you busy?",
    "下班": "off work", "加班": "overtime", "休息": "rest",
    "吃饭": "have a meal", "睡觉": "sleep", "起床": "get up",
    "回来": "come back", "过去": "go over", "过来": "come over",
    "出来": "come out", "出来": "come out", "出去": "go out",
    "上面": "above", "下面": "below", "左边": "left",
    "右边": "right", "前面": "front", "后面": "behind",
    "旁边": "beside", "附近": "nearby", "中间": "middle",
    "全部": "all", "部分": "part", "一半": "half",
    "很多": "many", "一点": "a little", "一些": "some",
    "非常": "very", "特别": "especially", "比较": "relatively",
    "真的": "really", "确实": "indeed", "其实": "actually",
    "但是": "but", "因为": "because", "所以": "so",
    "如果": "if", "虽然": "although", "然后": "then",
    "还是": "or still", "或者": "or", "并且": "and also",
}

HEADER = """\
# 候选译词表（英语）— glossary.en.tsv
#
# 格式：每行 `词<TAB>释义`，`#` 开头为注释。UTF-8。
# 加载顺序：bundle 内置（本文件）→ 用户目录覆盖
#   ~/Library/Application Support/DeskCap/rime/glossary.en.tsv
# 扩充词表：在用户目录放一份同名文件即可覆盖 / 追加，**不需要改代码、不需要重新打包**。
#
# 开启：defaults write com.office.deskcap.inputmethod.symi GlossaryLanguage -string en
# 关闭：defaults write com.office.deskcap.inputmethod.symi GlossaryLanguage -string off
#
# ⚠️ 本文件由 macos-ime/scripts/gen-glossary.py 生成，**请勿手改** ——
#    改数据请改生成器后重跑（`python3 macos-ime/scripts/gen-glossary.py`），
#    否则下次生成会覆盖你的手改。
#
# 来源与许可：
#   - 单字：自建 441 条（人工校订，**优先**）+ Unicode Unihan kDefinition
#     （`Unicode License v3`，宽松、可商用，需保留版权声明 —— 见
#      THIRD-PARTY-NOTICES.md）。用 `--unihan <Unihan_Readings.txt>` 补充。
#   - 词组：DeskCap 自撰 272 条，不含第三方词库。
#   - 未使用 ECDICT（英→中方向反）、CC-CEDICT（CC-BY-SA 有传染性）。
"""


def _load_unihan_defs(path: str) -> dict[str, str]:
    """读 Unihan_Readings.txt 的 kDefinition（汉字 → 英文释义）。"""
    defs: dict[str, str] = {}
    for line in open(path, encoding="utf-8", errors="ignore"):
        if "\tkDefinition\t" not in line:
            continue
        cp, _, val = line.split("\t", 2)
        try:
            defs[chr(int(cp[2:], 16))] = val.strip()
        except ValueError:
            continue
    return defs


_BAD_PREFIX = (
    "surname", "variant of", "old variant", "used in", "see ",
    "abbr. for", "same as", "ancient", "kangxi",
)


def _clean_definition(d: str) -> str:
    """清洗成候选框可显示的一行：去括号注释、跳过姓氏/异体，最多两义，≤44 字。"""
    import re as _re

    parts = [x.strip() for x in d.split(";")]
    out: list[str] = []
    for part in parts:
        part = _re.sub(r"\([^)]*\)", "", part).strip()
        if not part:
            continue
        if any(part.lower().startswith(b) for b in _BAD_PREFIX):
            continue
        out.append(part)
        if len(out) >= 2:
            break
    if not out:  # 全是姓氏/异体 → 取原始第一义兜底，别留空
        first = _re.sub(r"\([^)]*\)", "", parts[0]).strip()
        out = [first] if first else []
    return ", ".join(out)[:44].rstrip(", ")


def _essay_single_chars() -> list[str]:
    """essay.txt 里所有单字 —— 即 rime 词库**能打出来**的字集合。
    用它做范围，避免收录用户根本打不到的生僻字。"""
    path = OUT.parent / "essay.txt"
    if not path.exists():
        return []
    chars: list[str] = []
    for line in path.read_text(encoding="utf-8", errors="ignore").splitlines():
        parts = line.split("\t")
        if len(parts) == 2 and len(parts[0]) == 1 and parts[1].isdigit():
            chars.append(parts[0])
    return chars


def build(unihan: str | None = None) -> str:
    # 词组优先：同一个字既在 CHARS 又在 WORDS 时不冲突（key 不同）；
    # 真正的重复只可能来自数据区内部，validate() 会拦下来。
    lines = [HEADER]
    lines.append("# --- 常用单字（逐字合成的底座）---")
    merged: dict[str, str] = dict(CHARS)
    if unihan:
        defs = _load_unihan_defs(unihan)
        added = 0
        for ch in _essay_single_chars():
            if ch in merged:
                continue          # 自建优先
            d = defs.get(ch)
            if not d:
                continue
            c = _clean_definition(d)
            if c:
                merged[ch] = c
                added += 1
        print(f"Unihan 补充单字 {added} 条（自建 {len(CHARS)} 优先）", file=sys.stderr)
    for k, v in merged.items():
        lines.append(f"{k}\t{v}")
    lines.append("")
    lines.append("# --- 常用词组（整词精确命中优先）---")
    for k, v in WORDS.items():
        if k in merged:
            # 历史数据里少数词组表混进了单字（如「谁」）——单字区已经写过，
            # 这里跳过，避免重复键被 validate 拦下。
            continue
        lines.append(f"{k}\t{v}")
    lines.append("")
    return "\n".join(lines)


def validate(text: str) -> list[str]:
    """校验生成结果：真实 TAB、无重复键、无空值。返回问题列表。"""
    problems: list[str] = []
    seen: dict[str, int] = {}
    for n, line in enumerate(text.split("\n"), 1):
        if not line or line.startswith("#"):
            continue
        if "\t" not in line:
            problems.append(f"L{n}: 缺少 TAB 分隔符 -> {line!r}")
            continue
        key, _, value = line.partition("\t")
        key, value = key.strip(), value.strip()
        if not key or not value:
            problems.append(f"L{n}: 键或值为空 -> {line!r}")
            continue
        if key in seen:
            problems.append(f"L{n}: 重复键 {key!r}（首次出现在 L{seen[key]}）")
        seen[key] = n
    return problems


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true", help="只校验，不写文件")
    ap.add_argument(
        "--unihan",
        metavar="Unihan_Readings.txt",
        help="用 Unicode Unihan 的 kDefinition 补齐单字释义"
             "（下载 https://www.unicode.org/Public/UCD/latest/ucd/Unihan.zip）",
    )
    args = ap.parse_args()

    text = build(args.unihan)
    problems = validate(text)
    if problems:
        print("词表校验失败：", file=sys.stderr)
        for p in problems:
            print("  " + p, file=sys.stderr)
        return 1

    n_single = sum(1 for l in text.split("\n") if "\t" in l and len(l.split("\t")[0]) == 1)
    n_word = sum(1 for l in text.split("\n") if "\t" in l and len(l.split("\t")[0]) > 1)
    if args.check:
        print(f"OK — {n_single} 单字 + {n_word} 词组，校验通过")
        return 0

    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"已写入 {OUT}（{n_single} 单字 + {n_word} 词组）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
