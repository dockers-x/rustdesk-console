import { useEffect, useState } from "react";
import { UserCircle } from "@phosphor-icons/react";

function isPreviewableAvatar(src: string) {
  return (
    src.startsWith("http://") ||
    src.startsWith("https://") ||
    src.startsWith("data:image/") ||
    src.startsWith("/")
  );
}

export function AvatarPreview({
  src,
  alt,
  fallback,
  className = "size-12",
  iconSize = 28,
}: {
  src?: string;
  alt: string;
  fallback?: string;
  className?: string;
  iconSize?: number;
}) {
  const avatar = String(src ?? "").trim();
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setFailed(false);
  }, [avatar]);

  const showImage = avatar && isPreviewableAvatar(avatar) && !failed;

  return (
    <span
      className={`flex shrink-0 items-center justify-center overflow-hidden rounded-full border border-kumo-line bg-kumo-base text-kumo-brand ${className}`}
    >
      {showImage ? (
        <img
          src={avatar}
          alt={alt}
          className="size-full object-cover"
          onError={() => setFailed(true)}
        />
      ) : fallback ? (
        <span className="text-xs font-medium text-kumo-subtle">{fallback}</span>
      ) : (
        <UserCircle size={iconSize} weight="duotone" aria-hidden />
      )}
    </span>
  );
}
