import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App, { formatDuration } from "./App";

describe("DepthDeck viewer", () => {
  it("exposes the replay controls and local tape loader", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "DepthDeck" })).toBeInTheDocument();
    expect(screen.getByText("Load .ddt")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Play replay" })).toBeDisabled();
    expect(screen.getByLabelText("Replay position")).toBeInTheDocument();
  });

  it("formats monotonic time without consulting wall clock state", () => {
    expect(formatDuration(61_234)).toBe("01:01.234");
  });

  it("plays, pauses, changes speed, and seeks after a tape loads", async () => {
    render(<App />);
    const play = screen.getByRole("button", { name: "Play replay" });
    await waitFor(() => expect(play).toBeEnabled());

    fireEvent.click(play);
    expect(screen.getByRole("button", { name: "Pause replay" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "10×" }));
    expect(screen.getByRole("button", { name: "10×" })).toHaveClass("active");
    fireEvent.change(screen.getByLabelText("Replay position"), { target: { value: "500" } });
    expect(screen.getByLabelText("Replay position")).toHaveValue("500");
    expect(screen.getByRole("button", { name: "Play replay" })).toBeInTheDocument();
  });
});
