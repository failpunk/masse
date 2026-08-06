# Static page. Caddy rather than nginx because it takes $PORT without templating.
FROM caddy:2-alpine
COPY Caddyfile /etc/caddy/Caddyfile
COPY index.html /srv/index.html
COPY favicon.ico icon.svg icon-16.png icon-32.png icon-180.png icon-512.png /srv/
COPY og.png /srv/og.png
# This COPY list IS the site. A file added to site/ but not named here is simply
# never served, and the deploy still succeeds.
COPY robots.txt sitemap.xml /srv/
