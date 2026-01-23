# Algorithme de choix des jours Tempo - RTE

## Contexte

La Délibération de la CRE du 30 octobre 2014 confie à RTE la responsabilité du choix de la couleur des jours Tempo.

## Règles de placement

### Période
- Une année Tempo s'étend du **1er septembre** au **31 août** de l'année suivante

### Quotas
- **22 jours rouges** par an
- **43 jours blancs** par an

### Contraintes Rouge
- Tirés uniquement entre le **1er novembre** et le **31 mars**
- **Pas le weekend** (samedi/dimanche interdits)
- Maximum **5 jours rouges consécutifs**

### Contraintes Blanc
- Tirés **toute l'année** sauf le **dimanche**

## Données utilisées

### Consommation nette
```
C_nette = Consommation_nationale - (Prod_éolienne + Prod_photovoltaïque)
```

Calculée sur des journées type Tempo (06h - 06h le lendemain).

### Température
Utilisée pour la normalisation de la consommation.

## Normalisation

### Formule simplifiée (pédagogique)
```
Conso_nette_normalisée = (Conso_nette - 46050) / 2160
```

> NB: Valeurs basées sur l'historique 2014-2015, susceptibles de varier.

### Formule complète
```
C_nette_std = (C_nette - q_conso_0.4) / ((q_conso_0.8 - q_conso_0.4) × e^(-γ × (q_temp_0.3 - κ)))
```

Paramètres calibrés:
- **γ** (sensibilité température) = -0.1176
- **κ** (température moyenne Q30) = 8.3042°C

## Algorithme de décision

### Seuils dynamiques

Deux seuils qui dépendent de:
- `jour_tempo`: numéro du jour dans l'année Tempo (1er sept = jour 0)
- `stock_rouge`: jours rouges restants
- `stock_blanc`: jours blancs restants

| Seuil | A | B | C |
|-------|------|--------|--------|
| **Rouge** | 3.15 | -0.010 | -0.031 |
| **Blanc+Rouge** | 4.00 | -0.015 | -0.026 |

### Formules des seuils

```
Seuil_Rouge = 3.15 - 0.010 × jour_tempo - 0.031 × stock_rouge

Seuil_Blanc_Rouge = 4.00 - 0.015 × jour_tempo - 0.026 × (stock_rouge + stock_blanc)
```

### Logique de décision

```
SI Conso_nette_normalisée > Seuil_Rouge 
   ET contraintes_rouge_OK 
   ET stock_rouge > 0:
    → ROUGE

SINON SI Conso_nette_normalisée > Seuil_Blanc_Rouge 
   ET contraintes_blanc_OK 
   ET stock_blanc > 0:
    → BLANC

SINON:
    → BLEU
```

### Fin de période

Vérification de l'écoulement du stock pour assurer que l'intégralité soit placée.
L'algorithme peut placer des jours même si les seuils ne sont pas atteints.

## Implications pour la prédiction

### Ce qu'on connaît
- Date (donc jour_tempo, contraintes calendaires)
- Stocks actuels (estimables à partir de l'historique)

### Ce qu'on doit prédire
- **Consommation nette normalisée future**

### Approche optimale
1. Prédire la consommation nette (dépend de météo, activité économique)
2. Appliquer l'algorithme RTE déterministe sur cette prédiction

La vraie incertitude est sur la **consommation**, pas sur l'algorithme lui-même.

## Données sources

- Consommation: éCO2mix RTE
- Production éolien/solaire: éCO2mix RTE  
- Température: Météo-France / Open-Meteo
- Historique Tempo: API RTE publique
