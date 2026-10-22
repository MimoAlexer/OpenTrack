# syntax=docker/dockerfile:1
FROM postgres:18.6-bookworm@sha256:afc7e2d441324c0388fa80c3d24f733b4194a4eb7f47dd8ee2b08eb1a24a647c
RUN test "$(id -u postgres)" = 999 && test "$(id -g postgres)" = 999
COPY deploy/container-init-db.sh /docker-entrypoint-initdb.d/10-opentrack.sh
COPY deploy/init-db.sh deploy/provision-worker.sh deploy/provision-operator.sh \
    deploy/grant-worker.sh deploy/grant-worker.sql deploy/grant-operator.sh \
    deploy/grant-operator.sql /opt/opentrack/
RUN chmod 0555 /docker-entrypoint-initdb.d/10-opentrack.sh /opt/opentrack/*.sh
