#!/usr/bin/env python3
"""
Crinetics Pharmaceuticals ROI Calculator (CORRECTED)
Based on Summus Hex Dashboard methodology (Prospective ROI.yaml)
QA-verified against YAML source 2026-01-29

Usage:
1. Run Redshift queries to get condition_metrics and engagement_volume data
2. Load data into pandas DataFrames  
3. Call prospective_roi_analysis() with client parameters
"""

import pandas as pd
import numpy as np
from typing import Dict, List, Tuple


class SummusROICalculator:
    """ROI Calculator replicating Summus Hex Dashboard methodology (QA-verified)"""
    
    def __init__(self):
        # Dynamic cost assumptions (from YAML - CPT codes)
        self.dynamic_costs = {
            "primary_care": 121.0,  # CPT 99213
            "expert": 245.0,        # CPT 99204  
            "er": 1150.0            # CPT 99283
        }
        
        # Centralized ROI Constants (from YAML)
        self.roi_constants = {
            "er_factor": 0.04,   # Emergency Room utilization adjustment
            "hrs_smd": 12,       # Hours saved per Summus MD consult
            "hrs_exp": 60,       # Hours saved per Expert consult
            "hrs_nav": 15        # Hours saved per Navigation or PPR consult
        }
        
    def get_engagement_type_proportions(self, engagement_df: pd.DataFrame) -> Dict[str, float]:
        """Calculate engagement type proportions from engagement volume data"""
        relevant_types = ["Navigation", "SMD", "Expert", "Personalized Provider Referral"]
        
        # Filter to relevant engagement types
        df_eng = engagement_df[engagement_df["engagement_group"].isin(relevant_types)].copy()
        total_eng = df_eng["engagement_count"].sum()
        
        if total_eng == 0:
            return {etype: 0.0 for etype in relevant_types}
            
        proportions = {}
        for etype in relevant_types:
            count = df_eng.loc[df_eng["engagement_group"] == etype, "engagement_count"].sum()
            proportions[etype] = count / total_eng
            
        return proportions
    
    def calculate_total_savings(self, u: float, condition_df: pd.DataFrame, 
                              engagement_df: pd.DataFrame, members: int, 
                              wage: float, pepm: float) -> float:
        """
        Single source of truth for total savings calculation.
        CORRECTED to match YAML source exactly.
        
        Args:
            u: Utilization rate (e.g., 0.04 for 4%)
            condition_df: DataFrame with condition metrics from Redshift
            engagement_df: DataFrame with engagement volume from Redshift
            members: Number of eligible employees
            wage: Estimated hourly wage
            pepm: Price per employee per month
            
        Returns:
            Total projected savings (CTP + Avoided + Productivity)
        """
        # Get engagement proportions
        engagement_props = self.get_engagement_type_proportions(engagement_df)
        nav_pct = engagement_props.get("Navigation", 0)
        smd_pct = engagement_props.get("SMD", 0) 
        exp_pct = engagement_props.get("Expert", 0)
        ppr_pct = engagement_props.get("Personalized Provider Referral", 0)
        
        # Use dynamic costs
        cost_pcp = self.dynamic_costs["primary_care"]
        cost_expert = self.dynamic_costs["expert"]
        cost_er = self.dynamic_costs["er"]
        
        # Get ROI constants
        er_factor = self.roi_constants["er_factor"]
        hrs_smd = self.roi_constants["hrs_smd"]
        hrs_exp = self.roi_constants["hrs_exp"]
        hrs_nav = self.roi_constants["hrs_nav"]
        
        # =====================================================
        # 1. Changed Treatment Path (CTP) Savings
        # CORRECTED: Uses (smd_adj + exp_adj), not condition_volume * smd_adj
        # =====================================================
        ctp_total = 0.0
        smd_adj = smd_pct * u
        exp_adj = exp_pct * u
        
        for _, row in condition_df.iterrows():
            # Handle potential NaN values
            utilization_ratio = row.get('utilization_ratio', 0) or 0
            avg_ctp_rate = row.get('avg_ctp_rate', 0) or 0
            mean_ctp_savings = row.get('mean_ctp_savings', 0) or 0
            
            ctp_total += (
                members
                * (smd_adj + exp_adj)
                * utilization_ratio
                * avg_ctp_rate
                * mean_ctp_savings
            )
            
        # =====================================================
        # 2. Avoided Visits Savings
        # CORRECTED: Global engagement-based formula, not per-condition rates
        # =====================================================
        avoided_pcp = members * (smd_pct * u) * cost_pcp
        avoided_ex = members * (exp_pct * u) * cost_expert
        avoided_er = members * (smd_pct * u) * cost_er * er_factor
        avoided_total = avoided_pcp + avoided_ex + avoided_er
            
        # =====================================================
        # 3. Productivity Savings
        # CORRECTED: Using verified constants from YAML
        # =====================================================
        prod_smd = members * (smd_pct * u) * hrs_smd * wage
        prod_exp = members * (exp_pct * u) * hrs_exp * wage
        prod_nav = members * ((nav_pct + ppr_pct) * u) * hrs_nav * wage
        productivity_total = prod_smd + prod_exp + prod_nav
            
        return ctp_total + avoided_total + productivity_total
    
    def find_break_even_utilization(self, condition_df: pd.DataFrame, 
                                   engagement_df: pd.DataFrame, pepm: float,
                                   members: int, wage: float, 
                                   search_max: float = 0.15, step: float = 0.0005) -> float:
        """
        Find utilization rate where total savings equals annual cost (break-even point)
        """
        annual_cost = pepm * members * 12
        best_diff = float("inf")
        best_util = 0.0
        
        for x in np.arange(0, search_max + step, step):
            total_savings = self.calculate_total_savings(
                x, condition_df, engagement_df, members, wage, pepm
            )
            diff = abs(total_savings - annual_cost)
            if diff < best_diff:
                best_diff = diff
                best_util = x
                
        return best_util
    
    def calculate_savings_breakdown(self, u: float, condition_df: pd.DataFrame,
                                   engagement_df: pd.DataFrame, members: int,
                                   wage: float) -> Dict[str, float]:
        """Calculate individual savings components for detailed reporting"""
        engagement_props = self.get_engagement_type_proportions(engagement_df)
        nav_pct = engagement_props.get("Navigation", 0)
        smd_pct = engagement_props.get("SMD", 0)
        exp_pct = engagement_props.get("Expert", 0)
        ppr_pct = engagement_props.get("Personalized Provider Referral", 0)
        
        # CTP Savings
        smd_adj = smd_pct * u
        exp_adj = exp_pct * u
        ctp_total = 0.0
        for _, row in condition_df.iterrows():
            utilization_ratio = row.get('utilization_ratio', 0) or 0
            avg_ctp_rate = row.get('avg_ctp_rate', 0) or 0
            mean_ctp_savings = row.get('mean_ctp_savings', 0) or 0
            ctp_total += members * (smd_adj + exp_adj) * utilization_ratio * avg_ctp_rate * mean_ctp_savings
        
        # Avoided Visits
        er_factor = self.roi_constants["er_factor"]
        avoided_pcp = members * (smd_pct * u) * self.dynamic_costs["primary_care"]
        avoided_ex = members * (exp_pct * u) * self.dynamic_costs["expert"]
        avoided_er = members * (smd_pct * u) * self.dynamic_costs["er"] * er_factor
        
        # Productivity
        hrs_smd = self.roi_constants["hrs_smd"]
        hrs_exp = self.roi_constants["hrs_exp"]
        hrs_nav = self.roi_constants["hrs_nav"]
        prod_smd = members * (smd_pct * u) * hrs_smd * wage
        prod_exp = members * (exp_pct * u) * hrs_exp * wage
        prod_nav = members * ((nav_pct + ppr_pct) * u) * hrs_nav * wage
        
        return {
            "ctp_savings": ctp_total,
            "avoided_pcp": avoided_pcp,
            "avoided_specialist": avoided_ex,
            "avoided_er": avoided_er,
            "avoided_total": avoided_pcp + avoided_ex + avoided_er,
            "productivity_smd": prod_smd,
            "productivity_expert": prod_exp,
            "productivity_navigation": prod_nav,
            "productivity_total": prod_smd + prod_exp + prod_nav,
            "total_savings": ctp_total + (avoided_pcp + avoided_ex + avoided_er) + (prod_smd + prod_exp + prod_nav)
        }
    
    def generate_roi_summary(self, condition_df: pd.DataFrame, 
                            engagement_df: pd.DataFrame, pepm: float,
                            members: int, wage: float,
                            utilization_scenarios: List[float] = None) -> pd.DataFrame:
        """Generate ROI summary table for different utilization scenarios"""
        
        if utilization_scenarios is None:
            utilization_scenarios = [0.04, 0.05, 0.06]  # 4%, 5%, 6%
            
        annual_cost = pepm * members * 12
        summary_rows = []
        
        for util in utilization_scenarios:
            total_savings = self.calculate_total_savings(
                util, condition_df, engagement_df, members, wage, pepm
            )
            roi = (total_savings - annual_cost) / annual_cost if annual_cost > 0 else 0
            net_savings = total_savings - annual_cost
            
            summary_rows.append({
                "Utilization": f"{util:.0%}",
                "Annual_Cost": f"${annual_cost:,.0f}",
                "Total_Savings": f"${total_savings:,.0f}", 
                "Net_Savings": f"${net_savings:,.0f}",
                "ROI": f"{roi:.0%}",
                "Return_per_Dollar": f"${total_savings/annual_cost:.2f}" if annual_cost > 0 else "$0.00"
            })
            
        return pd.DataFrame(summary_rows)


def prospective_roi_analysis(condition_metrics: pd.DataFrame, 
                            engagement_volume: pd.DataFrame,
                            pepm: float, eligible_employees: int, 
                            estimated_hourly_wage: float) -> Dict:
    """
    Main function to run complete ROI analysis for prospect client
    
    Args:
        condition_metrics: Results from Redshift QUERY 1
        engagement_volume: Results from Redshift QUERY 2  
        pepm: Price per employee per month (e.g., 6.50)
        eligible_employees: Number of eligible employees (e.g., 600)
        estimated_hourly_wage: Estimated wage (e.g., 55.0)
        
    Returns:
        Dictionary with all ROI results and summaries
    """
    
    calculator = SummusROICalculator()
    
    # Basic calculations
    annual_cost = pepm * eligible_employees * 12
    
    # Break-even utilization  
    break_even = calculator.find_break_even_utilization(
        condition_metrics, engagement_volume, pepm, eligible_employees, estimated_hourly_wage
    )
    
    # ROI at 4% utilization (standard baseline)
    savings_4pct = calculator.calculate_total_savings(
        0.04, condition_metrics, engagement_volume, eligible_employees, estimated_hourly_wage, pepm
    )
    roi_4pct = (savings_4pct - annual_cost) / annual_cost if annual_cost > 0 else 0
    net_savings_4pct = savings_4pct - annual_cost
    
    # Detailed savings breakdown at 4%
    savings_breakdown = calculator.calculate_savings_breakdown(
        0.04, condition_metrics, engagement_volume, eligible_employees, estimated_hourly_wage
    )
    
    # ROI summary table
    roi_summary = calculator.generate_roi_summary(
        condition_metrics, engagement_volume, pepm, eligible_employees, estimated_hourly_wage
    )
    
    # Top opportunity conditions (create copy to avoid modifying original)
    condition_analysis = condition_metrics.copy()
    condition_analysis["opportunity_score"] = (
        eligible_employees * 0.04 * 
        condition_analysis["utilization_ratio"].fillna(0) * 
        condition_analysis["avg_ctp_rate"].fillna(0) * 
        condition_analysis["mean_ctp_savings"].fillna(0)
    )
    top_conditions = condition_analysis.nlargest(5, "opportunity_score")[
        ["condition", "utilization_ratio", "avg_ctp_rate", "mean_ctp_savings", "opportunity_score"]
    ]
    
    return {
        "client_params": {
            "pepm": pepm,
            "eligible_employees": eligible_employees,
            "estimated_hourly_wage": estimated_hourly_wage,
            "annual_cost": annual_cost
        },
        "key_metrics": {
            "break_even_utilization": break_even,
            "savings_at_4pct": savings_4pct,
            "roi_at_4pct": roi_4pct,
            "net_savings_at_4pct": net_savings_4pct,
            "return_per_dollar": savings_4pct / annual_cost if annual_cost > 0 else 0
        },
        "savings_breakdown_4pct": savings_breakdown,
        "roi_summary_table": roi_summary,
        "top_opportunities": top_conditions,
        "condition_data": condition_metrics,
        "engagement_data": engagement_volume
    }


def print_analysis_report(results: Dict, client_name: str = "Client"):
    """Print formatted analysis report"""
    print(f"\n{'='*60}")
    print(f"  {client_name} - Prospective ROI Analysis")
    print(f"{'='*60}\n")
    
    params = results['client_params']
    metrics = results['key_metrics']
    breakdown = results['savings_breakdown_4pct']
    
    print("CLIENT PARAMETERS:")
    print(f"  PEPM:                ${params['pepm']:.2f}")
    print(f"  Eligible Employees:  {params['eligible_employees']:,}")
    print(f"  Estimated Wage:      ${params['estimated_hourly_wage']:.2f}/hr")
    print(f"  Annual Cost:         ${params['annual_cost']:,.0f}")
    
    print(f"\nKEY METRICS (at 4% Utilization):")
    print(f"  Break-even:          {metrics['break_even_utilization']:.1%}")
    print(f"  Total Savings:       ${metrics['savings_at_4pct']:,.0f}")
    print(f"  Net Savings:         ${metrics['net_savings_at_4pct']:,.0f}")
    print(f"  ROI:                 {metrics['roi_at_4pct']:.0%}")
    print(f"  Return per $1:       ${metrics['return_per_dollar']:.2f}")
    
    print(f"\nSAVINGS BREAKDOWN (at 4%):")
    print(f"  CTP Savings:         ${breakdown['ctp_savings']:,.0f}")
    print(f"  Avoided Visits:      ${breakdown['avoided_total']:,.0f}")
    print(f"    - PCP:             ${breakdown['avoided_pcp']:,.0f}")
    print(f"    - Specialist:      ${breakdown['avoided_specialist']:,.0f}")
    print(f"    - ER:              ${breakdown['avoided_er']:,.0f}")
    print(f"  Productivity:        ${breakdown['productivity_total']:,.0f}")
    print(f"    - SMD:             ${breakdown['productivity_smd']:,.0f}")
    print(f"    - Expert:          ${breakdown['productivity_expert']:,.0f}")
    print(f"    - Navigation:      ${breakdown['productivity_navigation']:,.0f}")
    
    print(f"\nROI SUMMARY TABLE:")
    print(results['roi_summary_table'].to_string(index=False))
    
    print(f"\nTOP 5 OPPORTUNITY CONDITIONS:")
    print(results['top_opportunities'].to_string(index=False))
    print(f"\n{'='*60}\n")


# Example usage:
if __name__ == "__main__":
    print("ROI Calculator loaded. Use prospective_roi_analysis() with Redshift data.")
    print("Example:")
    print("""
    # Load data
    condition_metrics = pd.read_csv('condition_metrics.csv')
    engagement_volume = pd.read_csv('engagement_volume.csv')
    
    # Run analysis
    results = prospective_roi_analysis(
        condition_metrics=condition_metrics,
        engagement_volume=engagement_volume, 
        pepm=6.50,
        eligible_employees=600,
        estimated_hourly_wage=55.0
    )
    
    # Print report
    print_analysis_report(results, "Crinetics Pharmaceuticals")
    """)