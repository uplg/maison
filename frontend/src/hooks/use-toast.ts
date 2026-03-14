import { toast as sonnerToast } from "sonner";

type ToastVariant = "default" | "destructive";

interface ToastOptions {
  title?: string;
  description?: string;
  variant?: ToastVariant;
}

export function toast({ title, description, variant = "default" }: ToastOptions) {
  const message = title ?? "";

  if (variant === "destructive") {
    return sonnerToast.error(message, {
      description,
    });
  }

  return sonnerToast.success(message, {
    description,
  });
}

export function useToast() {
  return {
    toast,
    dismiss: sonnerToast.dismiss,
  };
}
