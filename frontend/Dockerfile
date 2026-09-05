FROM node:22-alpine AS builder
WORKDIR /app
COPY frontend/package.json frontend/pnpm-lock.yaml* ./
RUN corepack enable && corepack prepare pnpm@11.25.0 --activate && pnpm config set verify-deps-before-run false || true
ENV CI=true
RUN pnpm install --no-frozen-lockfile
COPY frontend ./
ARG VITE_API_BASE_URL=""
ENV VITE_API_BASE_URL=${VITE_API_BASE_URL}
RUN COREPACK_ENABLE_DOWNLOAD_PROMPT=0 pnpm build

FROM nginx:1.31-alpine
COPY --from=builder /app/dist /usr/share/nginx/html
COPY frontend/nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
