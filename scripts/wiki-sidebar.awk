# Builds _Sidebar.md from the converted Home.md: its section headings and the first link of each
# list item. Called by wiki.sh.

BEGIN { print "[Home](Home)" }
/^## / { print ""; print "**" substr($0, 4) "**"; print ""; next }
/^- / {
    i = index($0, "[")
    j = index($0, "](")
    if (i == 0 || j == 0) next
    rest = substr($0, j + 2)
    k = index(rest, ")")
    target = substr(rest, 1, k - 1)
    sp = index(target, " ")
    if (sp > 0) target = substr(target, 1, sp - 1)
    print "* " substr($0, i, j - i + 1) "(" target ")"
}
