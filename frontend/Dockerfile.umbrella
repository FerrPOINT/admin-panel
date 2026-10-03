# Umbrella build for the SDLC fleet root compose (context = /opt/dev/sdlc).
# Stages the sibling services-base frontend so the `@sdlc/ui` file dependency
# (../../services-base/frontend) resolves.
FROM node:26.10.0-bookworm-slim AS builder

RUN corepack enable && corepack prepare pnpm@10.28.1 --activate

WORKDIR /app
COPY admin-panel/frontend/package.json admin-panel/frontend/pnpm-lock.yaml admin-panel/frontend/pnpm-workspace.yaml ./
COPY services-base/frontend /services-base/frontend
COPY admin-panel/openapi/openapi.json /openapi/openapi.json
ENV CI=true COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN pnpm install --frozen-lockfile

COPY admin-panel/frontend/ ./
ARG VITE_PLATFORM_BRANDING_URL=""
ENV VITE_PLATFORM_BRANDING_URL=${VITE_PLATFORM_BRANDING_URL}
ARG VITE_PLATFORM_SERVICES_URL=""
ENV VITE_PLATFORM_SERVICES_URL=${VITE_PLATFORM_SERVICES_URL}
ARG VITE_AUTH_ISSUER="http://localhost:7701"
ENV VITE_AUTH_ISSUER=${VITE_AUTH_ISSUER}
RUN COREPACK_ENABLE_DOWNLOAD_PROMPT=0 pnpm build

FROM nginx:1.31-alpine
COPY --from=builder /app/dist /usr/share/nginx/html
COPY admin-panel/frontend/nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
