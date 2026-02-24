import { describe, expect, it } from "vitest";
import { detectPlanningIntent } from "./intent.js";

describe("detectPlanningIntent — true positives", () => {
  it("detects intent to build a new project management SaaS", () => {
    expect(detectPlanningIntent("I want to build a new project management SaaS")).toBe(true);
  });

  it("detects intent to plan architecture for a distributed caching system", () => {
    expect(detectPlanningIntent("Help me plan the architecture for a distributed caching system")).toBe(true);
  });

  it("detects intent to design a microservices platform from scratch", () => {
    expect(detectPlanningIntent("I need to design a microservices platform from scratch")).toBe(true);
  });

  it("detects intent to create a requirements doc for a new mobile app", () => {
    expect(detectPlanningIntent("Let's create a requirements doc for my new mobile app")).toBe(true);
  });

  it("detects intent to map out phases for a new project", () => {
    expect(detectPlanningIntent("I'm starting a new project — can you help me map out the phases?")).toBe(true);
  });

  it("detects intent to build out a roadmap for an analytics pipeline", () => {
    expect(detectPlanningIntent("We need to build out a roadmap for the analytics pipeline")).toBe(true);
  });

  it("detects requirements for building a real-time notification system", () => {
    expect(detectPlanningIntent("What are the requirements for building a real-time notification system?")).toBe(true);
  });
});

describe("detectPlanningIntent — false positives", () => {
  it("does not detect intent for a bug fix request", () => {
    expect(detectPlanningIntent("Can you fix this bug in my auth middleware?")).toBe(false);
  });

  it("does not detect intent for a simple CSS question", () => {
    expect(detectPlanningIntent("How do I center a div in CSS?")).toBe(false);
  });

  it("does not detect intent for a file operation", () => {
    expect(detectPlanningIntent("Read this file and summarize it")).toBe(false);
  });

  it("does not detect intent for task execution", () => {
    expect(detectPlanningIntent("Run the tests and show me the output")).toBe(false);
  });

  it("does not detect intent for a conceptual question", () => {
    expect(detectPlanningIntent("What is the difference between REST and GraphQL?")).toBe(false);
  });

  it("does not detect intent for a simple edit task", () => {
    expect(detectPlanningIntent("Update the README")).toBe(false);
  });

  it("does not detect intent for a code explanation request", () => {
    expect(detectPlanningIntent("Can you explain how this function works?")).toBe(false);
  });

  it("does not detect intent when 'building' appears in non-project context", () => {
    expect(detectPlanningIntent("I'm building momentum on this task")).toBe(false);
  });

  it("does not detect intent for a UI design task (not architecture)", () => {
    expect(detectPlanningIntent("Design a button that looks like this")).toBe(false);
  });

  it("does not detect intent for simple file creation", () => {
    expect(detectPlanningIntent("Create a new file called config.ts")).toBe(false);
  });
});
