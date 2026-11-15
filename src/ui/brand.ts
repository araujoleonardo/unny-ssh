import logo from "../assets/identidade/logo.png";
import compactLogo from "../assets/identidade/logo-reduzida.png";
import mark from "../assets/identidade/icone.png";

const variants = {
  full: { url: logo, width: 1448, height: 1086, viewBox: "230 425 1000 235" },
  compact: {
    url: compactLogo,
    width: 1254,
    height: 1254,
    viewBox: "350 420 550 370",
  },
  mark: { url: mark, width: 1254, height: 1254, viewBox: "225 250 800 800" },
};

/** Frame the supplied artwork's transparent margins without modifying the files. */
export function brand(
  variant: keyof typeof variants = "full",
  decorative = false,
) {
  const asset = variants[variant];
  return `<svg class="brand brand-${variant}" viewBox="${asset.viewBox}" ${decorative ? 'aria-hidden="true"' : 'role="img" aria-label="unny-ssh"'} focusable="false"><image href="${asset.url}" width="${asset.width}" height="${asset.height}" /></svg>`;
}
