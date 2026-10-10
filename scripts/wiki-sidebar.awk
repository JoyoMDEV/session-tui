# Builds _Sidebar.md from the converted Home.md. Each section heading becomes a link to that
# section on the Home page, and the sections that have wiki pages list them below it. Links that
# leave the wiki (to the README, say) are left out, because the Home page already has them and a
# sidebar that repeats them all is too long. Called by wiki.sh.

function slug(title,    s) {
    s = tolower(title)
    gsub(/[^a-z0-9 _-]/, "", s)
    gsub(/ /, "-", s)
    return s
}
function flush() {
    if (title == "") return
    print ""
    print "**[" title "](Home#" slug(title) ")**"
    if (items != "") { print ""; printf "%s", items }
    items = ""
}
BEGIN { print "[Home](Home)" }
/^## / { flush(); title = substr($0, 4); next }
/^- / {
    i = index($0, "[")
    j = index($0, "](")
    if (i == 0 || j == 0) next
    rest = substr($0, j + 2)
    target = substr(rest, 1, index(rest, ")") - 1)
    sp = index(target, " ")
    if (sp > 0) target = substr(target, 1, sp - 1)
    if (target ~ /^https?:/ || target ~ /^#/) next
    items = items "* " substr($0, i, j - i + 1) "(" target ")\n"
}
END { flush() }
