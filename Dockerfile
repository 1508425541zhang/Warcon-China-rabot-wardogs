# Svelte rendering and transport. All business runs in Dockerfile.native.
FROM node:24-bookworm-slim AS frontend-build
WORKDIR /app
RUN npm install --global bun@1.4.2
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile --ignore-scripts
COPY . .
RUN bun backend/tools/export-presentation-catalog.ts
RUN node node_modules/vite/bin/vite.js build
RUN bun install --frozen-lockfile --production --ignore-scripts

FROM node:24-bookworm-slim
WORKDIR /app
ENV NODE_ENV=production PORT=3000 HOST=0.0.0.0
COPY --from=frontend-build /app/package.json ./package.json
COPY --from=frontend-build /app/build ./build
COPY --from=frontend-build /app/node_modules ./node_modules
USER node
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s CMD node -e "fetch('http://127.0.0.1:3000/sign-in',{redirect:'manual'}).then(r=>process.exit(r.status<500?0:1)).catch(()=>process.exit(1))"
CMD ["node", "build/index.js"]
