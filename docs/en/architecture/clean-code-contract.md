# Architecture and clean-code contract

This document defines the **mandatory software-engineering contract** for every implementation in this project.

## 1. Clean-code principles

1. **Explicit responsibilities (SRP):** every module, class or package has a single reason to change.
2. **High cohesion and low coupling:** modules must be self-contained and interact only through abstract interfaces or typed contracts.
3. **Expressive domain names:** variables, functions and types must reflect the ubiquitous language of the business. Do not use generic names such as `manager`, `helper`, `utils` or `data`.
4. **Small, focused functions:** functions should perform a single logical action and ideally fit on one screen.
5. **Explicit errors:** silently suppressing exceptions is forbidden (`bare except`, ignoring errors). Every error must be handled, encapsulated or propagated with context.
6. **Zero speculative abstraction (YAGNI):** implement abstractions only when there are two or more proven concrete use cases.

## 2. Dependency direction (clean architecture)

- The dependency flow always points **inward**, toward the essential business rules.
- External mechanisms (databases, web frameworks, CLI, third-party libraries) are infrastructure details encapsulated by adapters.
- The application core is unaware of external protocols or specific cloud providers.

## 3. Modularity and decoupling

- No package or module may import its consumer.
- Dependency cycles are strictly forbidden and checked in the CI pipeline.
- Every repository folder must be self-explanatory and contain its own `README.md`.
