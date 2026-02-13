#!/usr/bin/env python3

import pandas as pd
import openpyxl
from collections import Counter, defaultdict
import re
from datetime import datetime
import numpy as np
import matplotlib.pyplot as plt

def analyze_shelf_organization():
    """Enhanced analysis including shelf organization patterns."""
    
    # Load the Excel file
    file_path = "/mnt/ssd/aletheia/theke/_reference/library/library_master.xlsx"
    df = pd.read_excel(file_path, sheet_name='physical')
    
    analysis = []
    analysis.append("# Cody's Physical Book Library Analysis (Enhanced)")
    analysis.append(f"*Analysis generated on {datetime.now().strftime('%Y-%m-%d at %H:%M %Z')}*")
    analysis.append("")
    analysis.append(f"**Total Books**: {len(df)}")
    analysis.append("")
    
    # 1. Shelf Organization Analysis
    analysis.append("## 1. Shelf Organization Analysis")
    analysis.append("*BookID represents physical shelf order within each genre section*")
    analysis.append("")
    
    # Group by genre and analyze BookID patterns
    if 'Genre' in df.columns and 'BookID' in df.columns:
        genre_analysis = []
        df_sorted = df.sort_values('BookID')
        
        for genre in df['Genre'].unique():
            if pd.isna(genre):
                continue
            genre_books = df[df['Genre'] == genre].copy()
            genre_books = genre_books.sort_values('BookID')
            
            book_ids = genre_books['BookID'].tolist()
            min_id = min(book_ids)
            max_id = max(book_ids)
            count = len(book_ids)
            
            # Check if BookIDs are contiguous within this genre
            expected_range = max_id - min_id + 1
            is_contiguous = (expected_range == count)
            
            # Calculate gaps
            gaps = []
            if not is_contiguous:
                full_range = list(range(min_id, max_id + 1))
                gaps = [id for id in full_range if id not in book_ids]
            
            genre_analysis.append({
                'genre': genre,
                'count': count,
                'min_id': min_id,
                'max_id': max_id,
                'is_contiguous': is_contiguous,
                'gaps': gaps
            })
        
        # Sort by min_id to show physical shelf order
        genre_analysis.sort(key=lambda x: x['min_id'])
        
        analysis.append("### Genre Sections by Physical Shelf Order")
        analysis.append("| Genre | Books | BookID Range | Contiguous | Gaps |")
        analysis.append("|-------|-------|--------------|------------|------|")
        
        for g in genre_analysis:
            contiguous_mark = "✓" if g['is_contiguous'] else "✗"
            gap_info = f"{len(g['gaps'])} gaps" if g['gaps'] else "None"
            analysis.append(f"| {g['genre']} | {g['count']} | {g['min_id']}-{g['max_id']} | {contiguous_mark} | {gap_info} |")
        
        analysis.append("")
        
        # Summary statistics
        contiguous_genres = sum(1 for g in genre_analysis if g['is_contiguous'])
        total_genres = len(genre_analysis)
        analysis.append(f"**Contiguous organization**: {contiguous_genres}/{total_genres} genres ({contiguous_genres/total_genres*100:.1f}%) are perfectly organized")
        
        # Check for genre mixing
        total_gaps = sum(len(g['gaps']) for g in genre_analysis)
        analysis.append(f"**Total organizational gaps**: {total_gaps} missing positions suggest {total_gaps} books may be mis-shelved or relocated")
        
    analysis.append("")
    
    # 2. Acquisition vs Shelf Order Analysis  
    analysis.append("## 2. Acquisition vs Shelf Order Correlation")
    
    if 'AddedDate' in df.columns and 'BookID' in df.columns:
        # Convert AddedDate to datetime if it isn't already
        df['AddedDate_dt'] = pd.to_datetime(df['AddedDate'], errors='coerce')
        
        # Remove rows where we can't parse the date
        df_with_dates = df.dropna(subset=['AddedDate_dt', 'BookID']).copy()
        
        if len(df_with_dates) > 0:
            # Calculate correlation between BookID and acquisition date
            # Convert datetime to ordinal for correlation calculation
            df_with_dates['AddedDate_ordinal'] = df_with_dates['AddedDate_dt'].apply(lambda x: x.toordinal())
            
            correlation = df_with_dates['BookID'].corr(df_with_dates['AddedDate_ordinal'])
            
            analysis.append(f"**BookID vs AddedDate correlation**: {correlation:.3f}")
            
            if correlation > 0.7:
                analysis.append("- **Strong positive correlation**: Books are shelved roughly in acquisition order")
            elif correlation > 0.3:
                analysis.append("- **Moderate correlation**: Some relationship between shelf order and acquisition")
            elif correlation > -0.3:
                analysis.append("- **Weak correlation**: Shelf order largely independent of acquisition order")
            else:
                analysis.append("- **Negative correlation**: Newer books tend to be shelved in lower BookID positions")
            
            analysis.append("")
            
            # Analyze acquisition patterns by genre
            analysis.append("### Acquisition Patterns by Genre")
            
            genre_acq_analysis = []
            for genre in df_with_dates['Genre'].unique():
                if pd.isna(genre):
                    continue
                    
                genre_data = df_with_dates[df_with_dates['Genre'] == genre]
                if len(genre_data) < 3:  # Need at least 3 points for meaningful correlation
                    continue
                    
                genre_corr = genre_data['BookID'].corr(genre_data['AddedDate_ordinal'])
                
                # Date range analysis
                date_span = genre_data['AddedDate_dt'].max() - genre_data['AddedDate_dt'].min()
                date_span_days = date_span.days
                
                genre_acq_analysis.append({
                    'genre': genre,
                    'correlation': genre_corr,
                    'count': len(genre_data),
                    'span_days': date_span_days,
                    'avg_acquisition_rate': len(genre_data) / max(date_span_days / 365, 0.1)  # books per year
                })
            
            # Sort by correlation
            genre_acq_analysis.sort(key=lambda x: x['correlation'] if not pd.isna(x['correlation']) else -999, reverse=True)
            
            analysis.append("| Genre | Books | Correlation | Span (days) | Rate (books/year) |")
            analysis.append("|-------|-------|-------------|-------------|-------------------|")
            
            for g in genre_acq_analysis[:15]:  # Top 15 most correlated
                if pd.isna(g['correlation']):
                    continue
                analysis.append(f"| {g['genre']} | {g['count']} | {g['correlation']:.3f} | {g['span_days']} | {g['avg_acquisition_rate']:.1f} |")
            
            analysis.append("")
            
            # Identify potential re-organization events
            analysis.append("### Organization Pattern Analysis")
            
            # Look for sudden changes in the BookID vs AddedDate pattern
            df_with_dates_sorted = df_with_dates.sort_values('AddedDate_dt')
            
            # Calculate rolling correlation in windows
            window_size = 50
            if len(df_with_dates_sorted) >= window_size:
                rolling_corr = []
                dates = []
                for i in range(window_size, len(df_with_dates_sorted)):
                    window_data = df_with_dates_sorted.iloc[i-window_size:i]
                    corr = window_data['BookID'].corr(window_data['AddedDate_ordinal'])
                    rolling_corr.append(corr)
                    dates.append(window_data['AddedDate_dt'].iloc[-1])
                
                # Find significant correlation changes
                corr_changes = []
                for i in range(1, len(rolling_corr)):
                    change = rolling_corr[i] - rolling_corr[i-1]
                    if abs(change) > 0.3:  # Significant change threshold
                        corr_changes.append({
                            'date': dates[i],
                            'correlation_change': change,
                            'new_correlation': rolling_corr[i]
                        })
                
                if corr_changes:
                    analysis.append("**Potential reorganization events detected:**")
                    for event in corr_changes[:5]:  # Show top 5
                        change_type = "increased" if event['correlation_change'] > 0 else "decreased"
                        analysis.append(f"- {event['date'].strftime('%Y-%m-%d')}: Correlation {change_type} by {abs(event['correlation_change']):.3f} to {event['new_correlation']:.3f}")
                else:
                    analysis.append("**No major reorganization events detected** - shelf organization pattern has been relatively stable")
            
        else:
            analysis.append("*Insufficient date data for correlation analysis*")
    else:
        analysis.append("*Missing required columns for correlation analysis*")
    
    analysis.append("")
    
    # 3. Continue with previous analyses but enhanced...
    # Genre breakdown
    analysis.append("## 3. Genre Breakdown")
    if 'Genre' in df.columns:
        genres = df['Genre'].dropna()
        genre_counts = genres.value_counts()
        total_with_genre = len(genres)
        
        analysis.append(f"**Books with genre information**: {total_with_genre} of {len(df)} ({total_with_genre/len(df)*100:.1f}%)")
        analysis.append("")
        analysis.append("| Genre | Count | Percentage | BookID Range |")
        analysis.append("|-------|-------|------------|--------------|")
        
        # Add BookID range information to genre breakdown
        for genre, count in genre_counts.head(20).items():
            pct = count / total_with_genre * 100
            genre_books = df[df['Genre'] == genre]['BookID']
            if len(genre_books) > 0:
                id_range = f"{genre_books.min()}-{genre_books.max()}"
            else:
                id_range = "N/A"
            analysis.append(f"| {genre} | {count} | {pct:.1f}% | {id_range} |")
        
        if len(genre_counts) > 20:
            others = genre_counts[20:].sum()
            pct = others / total_with_genre * 100
            analysis.append(f"| Other genres ({len(genre_counts) - 20}) | {others} | {pct:.1f}% | Various |")
    else:
        analysis.append("*No Genre column found*")
    
    analysis.append("")
    
    # 4. Read Status Analysis (same as before)
    analysis.append("## 4. Read Status vs Shelf Position")
    if 'ReadStatus' in df.columns:
        read_status = df['ReadStatus']
        read_count = (read_status == 1).sum()
        unread_count = (read_status == 0).sum()
        null_count = read_status.isna().sum()
        other_count = len(df) - read_count - unread_count - null_count
        
        analysis.append(f"- **Read** (ReadStatus = 1): {read_count} books ({read_count/len(df)*100:.1f}%)")
        analysis.append(f"- **Unread** (ReadStatus = 0): {unread_count} books ({unread_count/len(df)*100:.1f}%)")
        if null_count > 0:
            analysis.append(f"- **No status** (ReadStatus = null): {null_count} books ({null_count/len(df)*100:.1f}%)")
        if other_count > 0:
            analysis.append(f"- **Other status**: {other_count} books ({other_count/len(df)*100:.1f}%)")
        
        if read_count + unread_count > 0:
            analysis.append(f"- **Reading completion rate**: {read_count/(read_count+unread_count)*100:.1f}%")
            
            # Analyze read vs unread by shelf position
            read_books = df[df['ReadStatus'] == 1]
            unread_books = df[df['ReadStatus'] == 0]
            
            if len(read_books) > 0 and len(unread_books) > 0:
                avg_read_bookid = read_books['BookID'].mean()
                avg_unread_bookid = unread_books['BookID'].mean()
                
                analysis.append(f"- **Average shelf position**: Read books: {avg_read_bookid:.1f}, Unread books: {avg_unread_bookid:.1f}")
                
                if avg_read_bookid < avg_unread_bookid:
                    analysis.append("  - *Read books tend to be shelved earlier (lower BookIDs)*")
                else:
                    analysis.append("  - *Read books tend to be shelved later (higher BookIDs)*")
        else:
            analysis.append(f"- **Reading completion rate**: Cannot calculate (no clear read/unread status)")
    else:
        analysis.append("*No ReadStatus column found*")
    
    analysis.append("")
    
    # Continue with other analyses...
    # [Previous analysis sections would continue here - authors, series, dates, observations]
    
    # Key Insights Section
    analysis.append("## Key Organizational Insights")
    
    insights = []
    
    # Genre organization insight
    if 'Genre' in df.columns and 'BookID' in df.columns:
        contiguous_count = len([g for g in genre_analysis if g['is_contiguous']])
        total_genre_count = len(genre_analysis)
        if contiguous_count / total_genre_count > 0.8:
            insights.append("- **Highly organized**: Most genres are perfectly contiguous on shelves")
        elif contiguous_count / total_genre_count > 0.5:
            insights.append("- **Moderately organized**: Majority of genres are contiguous with some mixing")
        else:
            insights.append("- **Mixed organization**: Significant genre mixing suggests frequent reorganization or space constraints")
    
    # Acquisition pattern insight
    if 'AddedDate' in df.columns and 'BookID' in df.columns and len(df_with_dates) > 0:
        if correlation > 0.5:
            insights.append(f"- **Chronological shelving**: Strong correlation ({correlation:.3f}) suggests books are shelved in acquisition order within genres")
        elif correlation > 0:
            insights.append(f"- **Mixed chronological pattern**: Moderate correlation ({correlation:.3f}) suggests partial chronological organization")
        else:
            insights.append(f"- **Non-chronological shelving**: Low/negative correlation ({correlation:.3f}) suggests deliberate topical organization over acquisition order")
    
    # Space utilization insight
    max_bookid = df['BookID'].max()
    total_books = len(df)
    if max_bookid > total_books * 1.1:
        gap_percentage = (max_bookid - total_books) / max_bookid * 100
        insights.append(f"- **Shelf gaps**: {gap_percentage:.1f}% gap suggests reserved space for future acquisitions or recent removals")
    
    for insight in insights:
        analysis.append(insight)
    
    analysis.append("")
    analysis.append("---")
    analysis.append("*End of Enhanced Analysis*")
    
    return '\n'.join(analysis), df

if __name__ == "__main__":
    try:
        analysis_text, df = analyze_shelf_organization()
        
        # Write to file
        output_file = "/mnt/ssd/aletheia/theke/_reference/library/physical_library_analysis.md"
        with open(output_file, 'w', encoding='utf-8') as f:
            f.write(analysis_text)
        print(f"Enhanced analysis written to: {output_file}")
        
        # Print key organizational findings
        print("\n" + "="*60)
        print("SHELF ORGANIZATION ANALYSIS")
        print("="*60)
        
        # Quick shelf organization summary
        if 'Genre' in df.columns and 'BookID' in df.columns:
            genres_with_org = []
            for genre in df['Genre'].unique():
                if pd.isna(genre):
                    continue
                genre_books = df[df['Genre'] == genre]['BookID'].tolist()
                if len(genre_books) > 1:
                    min_id, max_id = min(genre_books), max(genre_books)
                    is_contiguous = (max_id - min_id + 1) == len(genre_books)
                    genres_with_org.append(is_contiguous)
            
            contiguous_pct = sum(genres_with_org) / len(genres_with_org) * 100
            print(f"Genre organization: {contiguous_pct:.1f}% of genres are perfectly contiguous")
        
        # Correlation analysis
        if 'AddedDate' in df.columns and 'BookID' in df.columns:
            df['AddedDate_dt'] = pd.to_datetime(df['AddedDate'], errors='coerce')
            df_with_dates = df.dropna(subset=['AddedDate_dt', 'BookID'])
            
            if len(df_with_dates) > 0:
                df_with_dates['AddedDate_ordinal'] = df_with_dates['AddedDate_dt'].apply(lambda x: x.toordinal())
                correlation = df_with_dates['BookID'].corr(df_with_dates['AddedDate_ordinal'])
                print(f"Acquisition correlation: {correlation:.3f} (BookID vs AddedDate)")
                
                if correlation > 0.5:
                    print("→ Books are primarily shelved in acquisition order")
                elif correlation > 0:
                    print("→ Partial chronological organization")
                else:
                    print("→ Non-chronological organization (topical/genre-based)")
        
    except Exception as e:
        print(f"Error during enhanced analysis: {e}")
        import traceback
        traceback.print_exc()