# Rewrites the relative links of one wiki page. Code blocks and inline code are copied as they are.
# Called by wiki.sh with rel (the page's path in the repository, so links can be resolved without
# touching the disk), repo (owner/repo) and ref (the tag or branch the docs come from).
# With mode=name it prints the wiki page name for rel and nothing else.

function normalize(path,    n, parts, i, depth, out, stack) {
    n = split(path, parts, "/")
    depth = 0
    for (i = 1; i <= n; i++) {
        if (parts[i] == "" || parts[i] == ".") continue
        if (parts[i] == "..") { if (depth > 0) depth--; continue }
        stack[++depth] = parts[i]
    }
    out = ""
    for (i = 1; i <= depth; i++) out = out (i > 1 ? "/" : "") stack[i]
    return out
}
function dirname(path,    i) {
    i = match(path, /\/[^\/]*$/)
    return i ? substr(path, 1, i - 1) : ""
}
function kind(name) {
    if (name == "adr") return "ADR"
    if (name == "how-to") return "How-to"
    return toupper(substr(name, 1, 1)) substr(name, 2)
}
function page(path,    rest, n, parts, i, out) {
    rest = substr(path, 6)
    sub(/\.md$/, "", rest)
    n = split(rest, parts, "/")
    if (n == 1) return rest
    out = kind(parts[1])
    for (i = 2; i <= n; i++) out = out "-" parts[i]
    return out
}
function rewrite(target,    hash, path, anchor, resolved) {
    if (target ~ /^[A-Za-z][A-Za-z0-9+.-]*:/ || target ~ /^#/) return target
    hash = index(target, "#")
    path = hash ? substr(target, 1, hash - 1) : target
    anchor = hash ? substr(target, hash) : ""
    resolved = normalize(dirname(rel) "/" path)
    if (resolved ~ /^docs\/.+\.md$/) return page(resolved) anchor
    return "https://github.com/" repo (path ~ /\/$/ ? "/tree/" : "/blob/") ref "/" resolved anchor
}
function rewrite_links(s,    out, i, j, t, sp, rest) {
    out = ""
    while ((i = index(s, "](")) > 0) {
        out = out substr(s, 1, i + 1)
        s = substr(s, i + 2)
        j = index(s, ")")
        if (j == 0) break
        t = substr(s, 1, j - 1)
        sp = index(t, " ")
        rest = ""
        if (sp > 0) { rest = substr(t, sp); t = substr(t, 1, sp - 1) }
        out = out rewrite(t) rest ")"
        s = substr(s, j + 1)
    }
    return out s
}
BEGIN {
    if (mode == "name") { print page(rel); exit }
}
{
    if ($0 ~ /^[ \t]*(```|~~~)/) { fenced = !fenced; print; next }
    if (fenced) { print; next }
    n = split($0, seg, "`")
    line = ""
    for (k = 1; k <= n; k++) {
        part = (k % 2 == 1) ? rewrite_links(seg[k]) : seg[k]
        line = line (k > 1 ? "`" : "") part
    }
    print line
}
