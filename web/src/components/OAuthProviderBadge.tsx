import {
  Fingerprint,
  GithubLogo,
  GoogleLogo,
  Keyhole,
  type Icon,
} from "@phosphor-icons/react";

export const OAUTH_PROVIDER_OPTIONS: {
  label: string;
  value: string;
  icon?: Icon;
  image?: string;
  shortLabel?: string;
}[] = [
  { label: "GitHub", value: "github", icon: GithubLogo },
  { label: "Google", value: "google", icon: GoogleLogo },
  { label: "OIDC", value: "oidc", icon: Fingerprint },
  {
    label: "LinuxDO",
    value: "linuxdo",
    image:
      "data:image/svg+xml;base64,PD94bWwgdmVyc2lvbj0iMS4wIiBlbmNvZGluZz0iVVRGLTgiPz48c3ZnIHZlcnNpb249IjEuMiIgYmFzZVByb2ZpbGU9InRpbnktcHMiIHdpZHRoPSIxMjgiIGhlaWdodD0iMTI4IiB2aWV3Qm94PSIwIDAgMTIwIDEyMCIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj48dGl0bGU+TElOVVggRE8gTG9nbzwvdGl0bGU+PGNsaXBQYXRoIGlkPSJhIj48Y2lyY2xlIGN4PSI2MCIgY3k9IjYwIiByPSI0NyIvPjwvY2xpcFBhdGg+PGNpcmNsZSBmaWxsPSIjZjBmMGYwIiBjeD0iNjAiIGN5PSI2MCIgcj0iNTAiLz48cmVjdCBmaWxsPSIjMWMxYzFlIiBjbGlwLXBhdGg9InVybCgjYSkiIHg9IjEwIiB5PSIxMCIgd2lkdGg9IjEwMCIgaGVpZ2h0PSIzMCIvPjxyZWN0IGZpbGw9IiNmMGYwZjAiIGNsaXAtcGF0aD0idXJsKCNhKSIgeD0iMTAiIHk9IjQwIiB3aWR0aD0iMTAwIiBoZWlnaHQ9IjQwIi8+PHJlY3QgZmlsbD0iI2ZmYjAwMyIgY2xpcC1wYXRoPSJ1cmwoI2EpIiB4PSIxMCIgeT0iODAiIHdpZHRoPSIxMDAiIGhlaWdodD0iMzAiLz48L3N2Zz4=",
  },
];

export function OAuthProviderBadge({ value }: { value: unknown }) {
  const raw = String(value ?? "").trim();
  const provider =
    OAUTH_PROVIDER_OPTIONS.find((p) => p.value === raw.toLowerCase()) ?? null;
  const Icon = provider?.icon;
  const label = (provider?.label ?? raw) || "OAuth";

  return (
    <span className="inline-flex min-h-7 items-center gap-2 rounded-md border border-kumo-line bg-kumo-elevated px-2 text-xs font-medium">
      {provider?.image ? (
        <img
          src={provider.image}
          alt=""
          className="size-4 rounded-full"
          aria-hidden="true"
        />
      ) : Icon ? (
        <Icon size={15} weight="regular" aria-hidden />
      ) : provider?.shortLabel ? (
        <span
          className="inline-flex h-4 min-w-6 items-center justify-center rounded bg-kumo-tint px-1 font-mono text-[10px] leading-none"
          aria-hidden="true"
        >
          {provider.shortLabel}
        </span>
      ) : (
        <Keyhole size={15} weight="regular" aria-hidden />
      )}
      <span>{label}</span>
    </span>
  );
}
