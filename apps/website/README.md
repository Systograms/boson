# Boson website

Public marketing site and documentation for Boson. Built with Next.js, Tailwind,
and shadcn — themed to match the Admin dashboard.

## Local development

```bash
npm install
npm run dev
```

Open [http://localhost:3000](http://localhost:3000).

Docs content lives in `content/docs/*.mdx`.

## Vercel

Deploy this directory as its own Vercel project:

| Setting | Value |
| --- | --- |
| Root Directory | `apps/website` |
| Framework | Next.js |
| Install Command | `npm ci` |
| Build Command | `npm run build` |

This app is not part of Docker Compose; it ships separately from the server,
worker, and dashboard.
