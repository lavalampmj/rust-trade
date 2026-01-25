/**
 * Configuration API client for Tauri commands.
 *
 * Provides a type-safe interface for CRUD operations on application configuration.
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  AppConfig,
  ConfigResponse,
  ValidationResult,
  ConfigAuditEntry,
} from "@/types/config";

/**
 * Configuration API interface.
 */
export interface ConfigAPI {
  // Full configuration
  getConfig(): Promise<ConfigResponse>;

  // Section operations
  getSection<T>(section: string): Promise<T>;
  updateSection<T>(section: string, value: T): Promise<void>;
  resetSection(section: string): Promise<void>;

  // Array item operations
  addItem<T>(section: string, item: T): Promise<void>;
  updateItem<T>(section: string, index: number, item: T): Promise<void>;
  removeItem(section: string, index: number): Promise<void>;

  // Validation
  validate(): Promise<ValidationResult>;
  validateSection<T>(section: string, value: T): Promise<ValidationResult>;

  // Persistence
  save(): Promise<void>;
  reload(): Promise<void>;

  // Audit
  getAuditLog(): Promise<ConfigAuditEntry[]>;

  // Debug
  getOverrides(): Promise<unknown>;
}

/**
 * Configuration API implementation using Tauri invoke.
 */
export const configApi: ConfigAPI = {
  /**
   * Get the full merged configuration.
   */
  async getConfig(): Promise<ConfigResponse> {
    return invoke<ConfigResponse>("get_config");
  },

  /**
   * Get a specific configuration section.
   *
   * @param section - Dot-notation path (e.g., "symbols", "accounts.default")
   */
  async getSection<T>(section: string): Promise<T> {
    return invoke<T>("get_config_section", { section });
  },

  /**
   * Update a configuration section.
   *
   * Changes are validated before being applied.
   *
   * @param section - Dot-notation path
   * @param value - New value for the section
   */
  async updateSection<T>(section: string, value: T): Promise<void> {
    return invoke("update_config_section", { section, value });
  },

  /**
   * Reset a configuration section to its default value.
   *
   * @param section - Dot-notation path
   */
  async resetSection(section: string): Promise<void> {
    return invoke("reset_config_section", { section });
  },

  /**
   * Add an item to an array section.
   *
   * @param section - Dot-notation path to the array (e.g., "symbols", "accounts.simulation")
   * @param item - Item to add
   */
  async addItem<T>(section: string, item: T): Promise<void> {
    return invoke("add_config_item", { section, item });
  },

  /**
   * Update an item in an array section by index.
   *
   * @param section - Dot-notation path to the array
   * @param index - Index of the item to update
   * @param item - New value for the item
   */
  async updateItem<T>(section: string, index: number, item: T): Promise<void> {
    return invoke("update_config_item", { section, index, item });
  },

  /**
   * Remove an item from an array section by index.
   *
   * @param section - Dot-notation path to the array
   * @param index - Index of the item to remove
   */
  async removeItem(section: string, index: number): Promise<void> {
    return invoke("remove_config_item", { section, index });
  },

  /**
   * Validate the current configuration.
   */
  async validate(): Promise<ValidationResult> {
    return invoke<ValidationResult>("validate_config");
  },

  /**
   * Validate a specific section value before applying.
   *
   * @param section - Dot-notation path
   * @param value - Value to validate
   */
  async validateSection<T>(section: string, value: T): Promise<ValidationResult> {
    return invoke<ValidationResult>("validate_config_section", {
      section,
      value,
    });
  },

  /**
   * Explicitly save configuration to disk.
   *
   * Note: Configuration is auto-saved after updates, but this can be used
   * to force a save (subject to rate limiting).
   */
  async save(): Promise<void> {
    return invoke("save_config");
  },

  /**
   * Reload configuration from disk.
   *
   * Useful if the config files were modified externally.
   */
  async reload(): Promise<void> {
    return invoke("reload_config");
  },

  /**
   * Get the configuration audit log.
   *
   * Returns a history of configuration changes.
   */
  async getAuditLog(): Promise<ConfigAuditEntry[]> {
    return invoke<ConfigAuditEntry[]>("get_config_audit_log");
  },

  /**
   * Get user overrides (for debugging).
   */
  async getOverrides(): Promise<unknown> {
    return invoke("get_config_overrides");
  },
};

// ============================================================================
// Convenience Functions
// ============================================================================

/**
 * Get trading symbols.
 */
export async function getSymbols(): Promise<string[]> {
  return configApi.getSection<string[]>("symbols");
}

/**
 * Set trading symbols.
 */
export async function setSymbols(symbols: string[]): Promise<void> {
  return configApi.updateSection("symbols", symbols);
}

/**
 * Add a trading symbol.
 */
export async function addSymbol(symbol: string): Promise<void> {
  return configApi.addItem("symbols", symbol);
}

/**
 * Remove a trading symbol by index.
 */
export async function removeSymbol(index: number): Promise<void> {
  return configApi.removeItem("symbols", index);
}

/**
 * Get default account configuration.
 */
export async function getDefaultAccount(): Promise<{
  id: string;
  currency: string;
  initial_balance: string;
}> {
  return configApi.getSection("accounts.default");
}

/**
 * Update default account configuration.
 */
export async function updateDefaultAccount(account: {
  id: string;
  currency: string;
  initial_balance: string;
}): Promise<void> {
  return configApi.updateSection("accounts.default", account);
}

/**
 * Get simulation accounts.
 */
export async function getSimulationAccounts(): Promise<
  Array<{
    id: string;
    currency: string;
    initial_balance: string;
    strategies: string[];
  }>
> {
  return configApi.getSection("accounts.simulation");
}

/**
 * Add a simulation account.
 */
export async function addSimulationAccount(account: {
  id: string;
  currency: string;
  initial_balance: string;
  strategies: string[];
}): Promise<void> {
  return configApi.addItem("accounts.simulation", account);
}

/**
 * Remove a simulation account by index.
 */
export async function removeSimulationAccount(index: number): Promise<void> {
  return configApi.removeItem("accounts.simulation", index);
}

/**
 * Get paper trading configuration.
 */
export async function getPaperTrading(): Promise<{
  enabled: boolean;
  strategy: string;
  initial_capital: number;
}> {
  return configApi.getSection("paper_trading");
}

/**
 * Update paper trading configuration.
 */
export async function updatePaperTrading(config: {
  enabled: boolean;
  strategy: string;
  initial_capital: number;
}): Promise<void> {
  return configApi.updateSection("paper_trading", config);
}

/**
 * Get RSI strategy parameters.
 */
export async function getRsiParams(): Promise<{
  period: number;
  oversold: number;
  overbought: number;
  order_quantity: string;
}> {
  return configApi.getSection("builtin_strategies.rsi");
}

/**
 * Update RSI strategy parameters.
 */
export async function updateRsiParams(params: {
  period: number;
  oversold: number;
  overbought: number;
  order_quantity: string;
  lookback_multiplier?: number;
  lookback_buffer?: number;
}): Promise<void> {
  return configApi.updateSection("builtin_strategies.rsi", params);
}

/**
 * Get SMA strategy parameters.
 */
export async function getSmaParams(): Promise<{
  short_period: number;
  long_period: number;
  order_quantity: string;
}> {
  return configApi.getSection("builtin_strategies.sma");
}

/**
 * Update SMA strategy parameters.
 */
export async function updateSmaParams(params: {
  short_period: number;
  long_period: number;
  order_quantity: string;
  lookback_multiplier?: number;
}): Promise<void> {
  return configApi.updateSection("builtin_strategies.sma", params);
}

/**
 * Get Python strategies.
 */
export async function getPythonStrategies(): Promise<
  Array<{
    id: string;
    file: string;
    class_name: string;
    description: string;
    enabled: boolean;
    sha256: string;
    limits: { max_execution_time_ms: number };
  }>
> {
  return configApi.getSection("strategies.python");
}

/**
 * Add a Python strategy.
 */
export async function addPythonStrategy(strategy: {
  id: string;
  file: string;
  class_name: string;
  description: string;
  enabled: boolean;
  limits?: { max_execution_time_ms: number };
}): Promise<void> {
  // Note: sha256 is computed on the backend, not provided here
  return configApi.addItem("strategies.python", {
    ...strategy,
    sha256: "",
    limits: strategy.limits ?? { max_execution_time_ms: 10 },
  });
}

/**
 * Update a Python strategy by index.
 */
export async function updatePythonStrategy(
  index: number,
  strategy: {
    id: string;
    file: string;
    class_name: string;
    description: string;
    enabled: boolean;
    limits?: { max_execution_time_ms: number };
  }
): Promise<void> {
  return configApi.updateItem("strategies.python", index, {
    ...strategy,
    sha256: "", // Will be recomputed on backend
    limits: strategy.limits ?? { max_execution_time_ms: 10 },
  });
}

/**
 * Remove a Python strategy by index.
 */
export async function removePythonStrategy(index: number): Promise<void> {
  return configApi.removeItem("strategies.python", index);
}

/**
 * Get fee configuration.
 */
export async function getFeesConfig(): Promise<{
  default_model: string;
  custom: { maker: string; taker: string };
}> {
  return configApi.getSection("execution.fees");
}

/**
 * Update fee model.
 */
export async function updateFeeModel(model: string): Promise<void> {
  const current = await getFeesConfig();
  return configApi.updateSection("execution.fees", {
    ...current,
    default_model: model,
  });
}

/**
 * Update custom fee rates.
 */
export async function updateCustomFees(fees: {
  maker: string;
  taker: string;
}): Promise<void> {
  const current = await getFeesConfig();
  return configApi.updateSection("execution.fees", {
    ...current,
    custom: fees,
  });
}

/**
 * Get alerting configuration.
 */
export async function getAlertingConfig(): Promise<{
  enabled: boolean;
  interval_secs: number;
  cooldown_secs: number;
}> {
  return configApi.getSection("alerting");
}

/**
 * Toggle alerting on/off.
 */
export async function toggleAlerting(enabled: boolean): Promise<void> {
  const current = await getAlertingConfig();
  return configApi.updateSection("alerting", { ...current, enabled });
}

// ============================================================================
// Default Export
// ============================================================================

export default configApi;
