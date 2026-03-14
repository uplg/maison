import { Toaster as Sonner } from "sonner";

export function Toaster() {
  return (
    <Sonner
      position="top-right"
      richColors
      closeButton
      toastOptions={{
        classNames: {
          toast: "border border-slate-200 bg-white text-slate-950 shadow-lg",
          title: "text-sm font-medium text-slate-950",
          description: "text-sm text-slate-500",
          actionButton: "bg-slate-900 text-white",
          cancelButton: "bg-slate-100 text-slate-700",
          error: "border-red-200 bg-red-50 text-red-950",
          success: "border-emerald-200 bg-emerald-50 text-emerald-950",
        },
      }}
    />
  );
}
