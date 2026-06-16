npns() {
    local output
    output=$(./target/release/npns 2>&1 >/dev/tty)
    local ret=$?
    local path="${output#NPNS_PATH:}"
    if [ $ret -eq 0 ] && [ -n "$path" ]; then
        cd "$path"
    else
        return 0
    fi
}