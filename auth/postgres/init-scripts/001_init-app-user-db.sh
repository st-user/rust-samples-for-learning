#!/bin/bash

set -e


psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    CREATE USER ${APP_USER} WITH ENCRYPTED PASSWORD '${APP_USER_PASSWORD}';
    CREATE DATABASE ${APP_DATABASE_NAME} WITH TEMPLATE = template0 ENCODING = 'UTF8' LC_COLLATE = '${DB_LANG}.utf8' LC_CTYPE = '${DB_LANG}.utf8';
EOSQL

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$APP_DATABASE_NAME" <<-EOSQL
    CREATE SCHEMA ${APP_SCHEMA};
    GRANT ALL PRIVILEGES ON SCHEMA ${APP_SCHEMA} TO ${APP_USER};
    ALTER SCHEMA ${APP_SCHEMA} OWNER TO ${APP_USER};
EOSQL

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$APP_DATABASE_NAME" <<-EOSQL
    CREATE TABLE IF NOT EXISTS ${APP_SCHEMA}.rooms (
        room_id bigserial PRIMARY KEY,
        room_name varchar(100) NOT NULL,
        secret_token varchar(120) NOT NULL
    );
    ALTER TABLE ${APP_SCHEMA}.rooms OWNER TO ${APP_USER};

    CREATE TABLE IF NOT EXISTS ${APP_SCHEMA}.members (
        member_id bigserial PRIMARY KEY,
        room_id bigint REFERENCES myappsch.rooms (room_id) ON DELETE CASCADE NOT NULL,
        member_name varchar(100) NOT NULL,
        secret_token varchar(120) NOT NULL
    );
    ALTER TABLE ${APP_SCHEMA}.members OWNER TO ${APP_USER};
EOSQL