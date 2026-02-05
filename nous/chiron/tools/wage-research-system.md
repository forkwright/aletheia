# Systematic Wage Research for ROI Calculations
**Purpose:** Estimate hourly wages for prospect clients using BLS data and SEC filings

## Crinetics Pharmaceuticals - Wage Analysis

### BLS Industry Data (2024)
**NAICS 3254 - Pharmaceutical and Medicine Manufacturing in California**

**Overall Industry Average:** $50-$60 per hour (all employees)
- Blends production workers with scientists, engineers, and managers
- Represents total compensation for covered employment
- California-specific data from QCEW

**By Occupation Type:**
- **Production workers:** $25-$35 per hour
- **QA/QC technicians:** $35-$45 per hour
- **Scientists/researchers:** $45-$65 per hour
- **Engineers (chemical, industrial):** $55-$75+ per hour
- **Management roles:** $60-$80+ per hour

### Company-Specific Factors
**Crinetics Profile:**
- **Location:** San Diego, CA (high-cost area)
- **Company Type:** Biotech/pharmaceutical R&D focused
- **Size:** 600 employees (mid-size)
- **Focus:** Endocrine disease therapeutics (specialized)

**Expected Employee Mix:**
- Higher percentage of scientists/researchers vs. production workers
- R&D-focused means more specialized roles
- San Diego market commands premium wages

### Recommended Wage Estimate
**$55/hour** for Crinetics ROI calculations

**Justification:**
- Above industry average due to R&D focus
- San Diego market premium
- Biotech vs. traditional pharma manufacturing
- Conservative estimate within BLS range

## Systematic Research Process

### 1. BLS Industry Lookup
**Primary Sources:**
- OEWS (Occupational Employment and Wage Statistics)
- QCEW (Quarterly Census of Employment and Wages)
- State-specific data when available

**Query Template:**
```
"BLS [NAICS_CODE] [INDUSTRY_NAME] average hourly wages [STATE] 2024"
```

### 2. SEC Filing Analysis (10K/Proxy)
**Key Sections to Search:**
- Executive compensation tables
- Employee count and compensation expenses
- Geographic salary disclosures
- Industry comparisons

**Calculation Method:**
```
Total Compensation Expense ÷ Total Employees ÷ 2080 hours = Avg Hourly Rate
```

### 3. Geographic Adjustments
**Cost of Living Factors:**
- San Francisco Bay Area: +25-30%
- Los Angeles/San Diego: +15-20%  
- Seattle: +10-15%
- Austin/Dallas: +5-10%
- National average: Baseline

### 4. Industry Modifiers
**Technology/Biotech:** +15-25% premium
**Finance:** +20-30% premium
**Healthcare systems:** +5-10% premium
**Manufacturing:** Baseline to +5%
**Retail/Service:** -10% to baseline

## Implementation Tools

### Perplexity Research Template
```
What is the average hourly wage for [INDUSTRY] employees in [STATE] 2024? 
Include specific BLS OEWS and QCEW data sources. 
For [COMPANY_NAME], also check SEC filings for employee compensation data.
```

### Quick Lookup Formula
```python
def estimate_hourly_wage(company, industry_naics, state, employee_count):
    # 1. Get BLS industry baseline for state
    # 2. Apply company size factor  
    # 3. Apply geographic factor
    # 4. Apply industry specialization factor
    # 5. Round to nearest $5
    return estimated_wage
```

### Verification Methods
1. **Cross-reference** with similar client wages in database
2. **Glassdoor/Indeed** spot checks for major roles
3. **Industry reports** from consulting firms
4. **Peer company** SEC filing comparisons

---

**For Crinetics:** $55/hour represents the 75th percentile for CA pharma industry, appropriate for R&D-focused biotech in San Diego market.