FROM postgres

ENV POSTGRES_USER=postgres
ENV POSTGRES_PASSWORD=postgrespass
ENV APP_USER=dbuser
ENV APP_USER_PASSWORD=mysecretpass
ENV APP_DATABASE_NAME=myappdb
ENV APP_SCHEMA=myappsch
ENV DB_LANG=ja_JP
ARG DB_LANG=ja_JP

RUN localedef -i ${DB_LANG} -c -f UTF-8 -A /usr/share/locale/locale.alias ${DB_LANG}.UTF-8

COPY init-scripts/* /docker-entrypoint-initdb.d/