import {
  CircleCheckIcon,
  InfoIcon,
  Loader2Icon,
  OctagonXIcon,
  TriangleAlertIcon,
} from "lucide-react";
import { Toaster as Sonner, type ToasterProps } from "sonner";

// No next-themes: Rust owns the theme and sets `.dark` on <html>, so the toast
// colors come from the theme tokens, which already follow that class.
const Toaster = ({ ...props }: ToasterProps) => {
  return (
    <Sonner
      className="toaster group [--border-radius:var(--radius-control)] [--normal-bg:var(--color-popover)] [--normal-border:var(--color-border)] [--normal-text:var(--color-popover-foreground)]"
      icons={{
        success: <CircleCheckIcon className="size-4" />,
        info: <InfoIcon className="size-4" />,
        warning: <TriangleAlertIcon className="size-4" />,
        error: <OctagonXIcon className="size-4" />,
        loading: <Loader2Icon className="size-4 animate-spin" />,
      }}
      {...props}
    />
  );
};

export { Toaster };
