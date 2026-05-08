#!/bin/bash
# Runs after the upstream 50-domjudge.sh has created/loaded the database.
#   - Enables Rust for judging+submission
#   - Resets the judgehost user password to a fixed value so the judgehost
#     container can authenticate using its env-var credentials
#   - Configures printing to dump submitted files into /var/log/domjudge/print.log
#
# Idempotent: safe to run on every container start.

set -eu
cd /opt/domjudge/domserver

mariadb_args=(
    -h "$MYSQL_HOST"
    -P "${MYSQL_PORT:-3306}"
    -u "$MYSQL_USER"
    "-p$MYSQL_PASSWORD"
    "$MYSQL_DATABASE"
    --silent
    --skip-column-names
)

# Stream a SQL statement on stdin so we never have to escape values into -e.
run_sql() {
    mariadb "${mariadb_args[@]}"
}

# 1) Wait for the DB.
for _ in $(seq 1 30); do
    if echo "SELECT 1;" | run_sql >/dev/null 2>&1; then break; fi
    sleep 1
done

# 2) Enable Rust.
echo "[..] Enabling Rust language"
echo "UPDATE language
      SET allow_judge=1, allow_submit=1, time_factor=GREATEST(time_factor, 3)
      WHERE langid='rs';" | run_sql
echo "[ok] Rust enabled"

# 3) Reset known passwords for `admin` and `judgehost` so the stack always
#    starts in a known state, even after a container recreate where the
#    upstream image lost its `initial_admin_password.secret` value.
reset_password() {
    local user="$1"; local pass="$2"
    PASS="$pass" \
    HASH="$(PASS="$pass" php -r 'echo password_hash(getenv("PASS"), PASSWORD_BCRYPT);')"
    {
        printf "UPDATE user SET password='"
        printf '%s' "$HASH"
        printf "' WHERE username='%s';\n" "$user"
    } | run_sql
}

reset_password admin "${INITIAL_ADMIN_PASSWORD:-admin}"
reset_password judgehost "${INITIAL_JUDGEHOST_PASSWORD:-judgehost}"
# `demo` is the seeded team account in the example "Demo contest".
reset_password demo "${INITIAL_DEMO_PASSWORD:-demo}"
echo "[ok] admin/judgehost/demo passwords reset"

# 4) Configure printing.
mkdir -p /var/log/domjudge
chown www-data:www-data /var/log/domjudge
chmod 0755 /var/log/domjudge

# Configuration values are stored as JSON; we set the command to invoke a
# wrapper script mounted at /dj-print/print.sh from the host.
echo "[..] Configuring printing"
echo 'REPLACE INTO configuration (name, value) VALUES
        ("print_command", JSON_QUOTE("/dj-print/print.sh [file]")),
        ("enable_printing", "true");' | run_sql
echo "[ok] printing configured"
