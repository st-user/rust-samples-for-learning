FROM node AS build
WORKDIR /app
COPY package* ./
COPY webpack.config.js ./
COPY tsconfig.json ./
RUN npm install
COPY src ./src
RUN npm run build
COPY public/* ./dist/

FROM nginx:alpine
COPY --from=build /app/dist usr/share/nginx/html
